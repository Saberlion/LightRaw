use crate::platform::android::is_android_content_uri;
#[cfg(target_os = "android")]
use crate::platform::android::{
    get_android_cached_lut_path, read_android_content_uri, resolve_android_content_uri_name,
};
use anyhow::anyhow;
use image::{DynamicImage, GenericImageView, Rgb, Rgb32FImage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, copy, create_dir_all, read_dir};
use std::io::{BufRead, BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use base64::{Engine as _, engine::general_purpose};
use mozjpeg_rs::{Encoder, Preset};
use serde_json::Value;
use tauri::{AppHandle, Manager, State, ipc::Channel};

#[cfg(any(target_os = "android", target_env = "ohos"))]
use include_dir::{Dir, include_dir};

/// Built-in LUTs are bundled as Tauri resources on desktop, but the mobile
/// platforms cannot resolve `BaseDirectory::Resource` (on OHOS the tauri fork's
/// resource resolver returns `UnsupportedPlatform`, and the HAP does not ship
/// `src-tauri/resources` at all). Embed them in the binary and unpack once into
/// app_data_dir, mirroring lens_correction.rs.
#[cfg(any(target_os = "android", target_env = "ohos"))]
static BUILTIN_LUTS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/resources/film_luts");

use crate::app::state::AppState;
use crate::decode::as_shot::as_shot;
use crate::decode::baseline_exposure::BaselineOptions;
use crate::export::tasks::ensure_export_not_cancelled;
use crate::masks::generation::{MaskDefinition, get_cached_or_generate_masks};
use crate::render::gpu::context::GpuContext;
use crate::render::gpu::output::process_and_get_dynamic_image;
use crate::render::gpu::processor::RenderRequest;
use crate::render::options::{RenderOptions, resolve_render_options_from_handle};
use crate::render::preview::prepare_preview_source;
use crate::render::source::RenderSource;
use crate::render::uniforms::{AllAdjustments, get_all_adjustments_from_json};

#[derive(Debug, Clone)]
pub struct Lut {
    pub size: u32,
    pub data: Vec<f32>,
}

impl Lut {
    fn byte_size(&self) -> usize {
        self.data.len() * std::mem::size_of::<f32>()
    }

    fn texel(&self, r: usize, g: usize, b: usize) -> [f32; 3] {
        let size = self.size as usize;
        let i = ((b * size + g) * size + r) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    fn sample_tetrahedral(&self, rgb: [f32; 3]) -> [f32; 3] {
        let max = (self.size - 1) as f32;
        let mut base = [0usize; 3];
        let mut next = [0usize; 3];
        let mut f = [0f32; 3];
        for c in 0..3 {
            let scaled = rgb[c].clamp(0.0, 1.0) * max;
            let floor = scaled.floor();
            base[c] = floor as usize;
            next[c] = (base[c] + 1).min(self.size as usize - 1);
            f[c] = scaled - floor;
        }
        let at = |r: bool, g: bool, b: bool| {
            self.texel(
                if r { next[0] } else { base[0] },
                if g { next[1] } else { base[1] },
                if b { next[2] } else { base[2] },
            )
        };
        let (fr, fg, fb) = (f[0], f[1], f[2]);
        let c000 = at(false, false, false);
        let c111 = at(true, true, true);
        let (w, c1, c2) = if fr > fg {
            if fg > fb {
                (
                    [1.0 - fr, fr - fg, fg - fb, fb],
                    at(true, false, false),
                    at(true, true, false),
                )
            } else if fr > fb {
                (
                    [1.0 - fr, fr - fb, fb - fg, fg],
                    at(true, false, false),
                    at(true, false, true),
                )
            } else {
                (
                    [1.0 - fb, fb - fr, fr - fg, fg],
                    at(false, false, true),
                    at(true, false, true),
                )
            }
        } else if fb > fg {
            (
                [1.0 - fb, fb - fg, fg - fr, fr],
                at(false, false, true),
                at(false, true, true),
            )
        } else if fb > fr {
            (
                [1.0 - fg, fg - fb, fb - fr, fr],
                at(false, true, false),
                at(false, true, true),
            )
        } else {
            (
                [1.0 - fg, fg - fr, fr - fb, fb],
                at(false, true, false),
                at(true, true, false),
            )
        };
        std::array::from_fn(|c| c000[c] * w[0] + c1[c] * w[1] + c2[c] * w[2] + c111[c] * w[3])
    }

    fn resampled(&self, size: u32) -> Lut {
        if self.size <= size {
            return self.clone();
        }
        let step = 1.0 / (size - 1) as f32;
        let mut data = Vec::with_capacity((size * size * size * 3) as usize);
        for b in 0..size {
            for g in 0..size {
                for r in 0..size {
                    data.extend(self.sample_tetrahedral([
                        r as f32 * step,
                        g as f32 * step,
                        b as f32 * step,
                    ]));
                }
            }
        }
        Lut { size, data }
    }
}

pub const LUT_CACHE_BUDGET_BYTES: usize = 256 * 1024 * 1024;
pub const SWATCH_LUT_CACHE_BUDGET_BYTES: usize = 64 * 1024 * 1024;
const SWATCH_LUT_SIZE: u32 = 33;

pub struct LutCache {
    entries: HashMap<String, (Arc<Lut>, u64)>,
    loading: HashMap<String, Arc<Mutex<()>>>,
    clock: u64,
    bytes: usize,
    budget: usize,
}

impl LutCache {
    pub fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            loading: HashMap::new(),
            clock: 0,
            bytes: 0,
            budget,
        }
    }

    fn get(&mut self, path: &str) -> Option<Arc<Lut>> {
        self.clock += 1;
        let clock = self.clock;
        self.entries.get_mut(path).map(|(lut, last_used)| {
            *last_used = clock;
            lut.clone()
        })
    }

    fn peek(&self, path: &str) -> Option<Arc<Lut>> {
        self.entries.get(path).map(|(lut, _)| lut.clone())
    }

    fn insert(&mut self, path: String, lut: Arc<Lut>) {
        self.clock += 1;
        self.bytes += lut.byte_size();
        if let Some((old, _)) = self.entries.insert(path, (lut, self.clock)) {
            self.bytes -= old.byte_size();
        }
        while self.bytes > self.budget && self.entries.len() > 1 {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, last_used))| *last_used)
                .map(|(path, _)| path.clone())
            else {
                break;
            };
            if let Some((evicted, _)) = self.entries.remove(&oldest) {
                self.bytes -= evicted.byte_size();
            }
        }
    }

    fn insert_if_room(&mut self, path: &str, lut: Arc<Lut>) {
        if !self.entries.contains_key(path) && self.bytes + lut.byte_size() <= self.budget {
            self.insert(path.to_string(), lut);
        }
    }
}

fn get_or_load(
    cache: &Mutex<LutCache>,
    path: &str,
    load: impl FnOnce() -> Result<Lut, String>,
) -> Result<Arc<Lut>, String> {
    let gate = {
        let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(lut) = cache.get(path) {
            return Ok(lut);
        }
        cache.loading.entry(path.to_string()).or_default().clone()
    };
    let _loading = gate.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(lut) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(path) {
        return Ok(lut);
    }
    let result = load().map(Arc::new);
    let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
    cache.loading.remove(path);
    if let Ok(lut) = &result {
        cache.insert(path.to_string(), lut.clone());
    }
    result
}

fn get_swatch_lut(state: &State<AppState>, path: &str) -> Result<Arc<Lut>, String> {
    get_or_load(&state.swatch_lut_cache, path, || {
        let cached = state
            .lut_cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .peek(path);
        let full = match cached {
            Some(lut) => lut,
            None => {
                let lut = Arc::new(parse_lut_file(path).map_err(|e| e.to_string())?);
                state
                    .lut_cache
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert_if_room(path, lut.clone());
                lut
            }
        };
        Ok(full.resampled(SWATCH_LUT_SIZE))
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LutEntry {
    pub name: String,
    pub path: String,
    pub is_built_in: bool,
    /// Which shader slot this LUT belongs to: scene-referred LUTs are sampled
    /// with the working colour V-Log-encoded (before the tone curve), display
    /// LUTs with the final sRGB signal (after the curves). Packed with the
    /// entry because the old "built-in implies scene-referred" guess breaks
    /// for display-domain packs (e.g. the sRGB -> sRGB camera styles).
    pub is_scene_referred: bool,
    /// Human-readable name from manifest.json displayNames; falls back to the
    /// file stem when the manifest does not list the file.
    pub display_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LutPreviewRequest {
    pub path: String,
    #[serde(default)]
    pub is_scene_referred: bool,
}

#[derive(Serialize)]
pub struct LutParseResult {
    pub size: u32,
}

#[derive(Serialize)]
pub struct LutPreview {
    pub path: String,
    pub thumb: Option<String>,
}

fn strip_verbatim(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_string())
}

fn film_luts_dir(app_handle: &AppHandle) -> Option<PathBuf> {
    #[cfg(any(target_os = "android", target_env = "ohos"))]
    if let Some(dir) = extract_builtin_luts(app_handle) {
        return Some(dir);
    }

    app_handle
        .path()
        .resolve("resources/film_luts", tauri::path::BaseDirectory::Resource)
        .ok()
        .map(|p| strip_verbatim(&p))
}

/// Unpack the embedded LUT set into `app_data_dir/builtin_luts`.
///
/// The stamp covers crate version + file count + total bytes, so shipping a
/// different LUT set re-extracts on the next launch without touching the user's
/// own LUT directory.
#[cfg(any(target_os = "android", target_env = "ohos"))]
fn extract_builtin_luts(app_handle: &AppHandle) -> Option<PathBuf> {
    let dir = crate::app_paths::app_data_dir(app_handle)
        .ok()?
        .join("builtin_luts");
    let stamp = format!(
        "{}:{}:{}",
        env!("CARGO_PKG_VERSION"),
        BUILTIN_LUTS.files().count(),
        BUILTIN_LUTS
            .files()
            .map(|f| {
                let name = f
                    .path()
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                // include the name so a rename re-extracts even if count and
                // total bytes happen to stay the same
                format!("{name}:{}", f.contents().len())
            })
            .collect::<Vec<_>>()
            .join("|")
    );
    let marker = dir.join(".stamp");

    if let Ok(existing) = std::fs::read_to_string(&marker)
        && existing == stamp
    {
        return Some(dir);
    }

    if let Err(e) = create_dir_all(&dir) {
        log::warn!("built-in LUT dir {:?} is unusable: {e}", dir);
        return None;
    }

    // Remove files that are no longer part of the embedded set (e.g. after a
    // rename), so the directory is an exact mirror of the pack.
    let embedded: std::collections::HashSet<&str> = BUILTIN_LUTS
        .files()
        .filter_map(|f| f.path().file_name().and_then(|s| s.to_str()))
        .collect();
    if let Ok(existing) = read_dir(&dir) {
        for entry in existing.flatten() {
            let name = entry.file_name();
            if let Some(name_str) = name.to_str()
                && !embedded.contains(name_str)
                && name_str != ".stamp"
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    let mut written = 0usize;
    for file in BUILTIN_LUTS.files() {
        let Some(name) = file.path().file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Err(e) = std::fs::write(dir.join(name), file.contents()) {
            log::warn!("failed to extract built-in LUT {name}: {e}");
            continue;
        }
        written += 1;
    }

    if let Err(e) = std::fs::write(&marker, &stamp) {
        log::warn!("failed to write built-in LUT stamp: {e}");
    }
    log::info!("extracted {written} built-in LUTs to {:?}", dir);
    Some(dir)
}

pub fn get_luts_dir(app_data_dir: &Path) -> anyhow::Result<PathBuf> {
    let luts_dir = app_data_dir.join("luts");
    if !luts_dir.exists() {
        create_dir_all(&luts_dir)?;
    }
    Ok(luts_dir)
}

/// Optional `<luts dir>/manifest.json` declaring which files are
/// scene-referred. Without it the historical "built-in implies
/// scene-referred" guess applies (correct for the V-Log spektrafilm set).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LutDirManifest {
    #[serde(default)]
    scene_referred: Vec<String>,
    /// file name -> display name (falls back to the file stem when absent).
    #[serde(default)]
    display_names: std::collections::HashMap<String, String>,
}

impl LutDirManifest {
    fn load(dir: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
        match serde_json::from_str(&text) {
            Ok(manifest) => Some(manifest),
            Err(e) => {
                log::warn!("ignoring invalid LUT manifest in {dir:?}: {e}");
                None
            }
        }
    }

    fn is_scene_referred(&self, file_name: &str, file_stem: &str) -> bool {
        self.scene_referred.iter().any(|listed| {
            listed.eq_ignore_ascii_case(file_name) || listed.eq_ignore_ascii_case(file_stem)
        })
    }
}

pub fn list_luts_in_dir(dir: &Path, is_built_in: bool) -> anyhow::Result<Vec<LutEntry>> {
    let mut entries: Vec<LutEntry> = Vec::new();
    if !dir.exists() {
        return Ok(entries);
    }
    let manifest = LutDirManifest::load(dir);
    for entry in read_dir(dir)? {
        let path = entry?.path();
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        let is_supported = matches!(
            extension.as_str(),
            "cube" | "3dl" | "png" | "jpg" | "jpeg" | "tiff"
        );

        if is_supported {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("LUT")
                .to_string();

            let is_scene_referred = match &manifest {
                Some(manifest) => manifest.is_scene_referred(
                    path.file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or_default(),
                    &name,
                ),
                // Historical fallback, only correct for the V-Log built-in set.
                None => is_built_in,
            };

            let file_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            let display_name = manifest
                .as_ref()
                .and_then(|m| m.display_names.get(&file_name).cloned())
                .unwrap_or_else(|| name.clone());

            entries.push(LutEntry {
                name,
                path: strip_verbatim(&path).to_string_lossy().into_owned(),
                is_built_in,
                is_scene_referred,
                display_name,
            });
        }
    }
    entries.sort_by_key(|a| a.name.to_lowercase());
    Ok(entries)
}

pub fn unique_destination(dir: &Path, stem: &str, extension: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{}.{}", stem, extension));
    let mut suffix = 1;
    while candidate.exists() && suffix < 1000 {
        candidate = dir.join(format!("{} ({}).{}", stem, suffix, extension));
        suffix += 1;
    }
    candidate
}

pub fn import_luts_to_dir(dir: &Path, source_paths: &[String]) -> anyhow::Result<Vec<LutEntry>> {
    for source in source_paths {
        if let Err(error) = parse_lut_file(source) {
            log::warn!("Skipping invalid LUT '{}': {}", source, error);
            continue;
        }

        #[cfg(target_os = "android")]
        if is_android_content_uri(source) {
            if let Err(error) = import_android_lut(source) {
                log::error!("Failed to import LUT from '{}': {}", source, error);
            }
            continue;
        }

        let source_path = Path::new(source);
        let stem = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("LUT");
        let extension = source_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("cube")
            .to_lowercase();
        let destination = unique_destination(dir, stem, &extension);
        if let Err(error) = copy(source_path, &destination) {
            log::error!("Failed to copy LUT '{}': {}", source, error);
        }
    }
    list_luts_in_dir(dir, false)
}

#[cfg(target_os = "android")]
fn import_android_lut(source: &str) -> anyhow::Result<()> {
    let resolved_name = resolve_android_content_uri_name(source)
        .map_err(|e| anyhow!("Failed to resolve content URI: {}", e))?;
    let stem = Path::new(&resolved_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("LUT")
        .to_string();
    let extension = Path::new(&resolved_name)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("cube")
        .to_lowercase();
    let bytes = read_android_content_uri(source)
        .map_err(|e| anyhow!("Failed to read content URI: {}", e))?;

    let cache_path = get_android_cached_lut_path(source, &extension)?;
    let cache_dir = cache_path
        .parent()
        .ok_or_else(|| anyhow!("Invalid cache path"))?
        .to_path_buf();
    let destination = unique_destination(&cache_dir, &stem, &extension);
    std::fs::write(&destination, &bytes)?;
    Ok(())
}

fn parse_cube(reader: impl BufRead) -> anyhow::Result<Lut> {
    let mut size: Option<u32> = None;
    let mut data: Vec<f32> = Vec::new();
    let mut line_num = 0;

    for line in reader.lines() {
        line_num += 1;
        let line = line?;
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0].to_uppercase().as_str() {
            "TITLE" | "DOMAIN_MIN" | "DOMAIN_MAX" => continue,

            "LUT_3D_SIZE" => {
                if parts.len() < 2 {
                    return Err(anyhow!(
                        "Malformed LUT_3D_SIZE on line {}: '{}'",
                        line_num,
                        line
                    ));
                }
                size = Some(parts[1].parse().map_err(|e| {
                    anyhow!(
                        "Failed to parse LUT_3D_SIZE on line {}: '{}'. Error: {}",
                        line_num,
                        line,
                        e
                    )
                })?);
            }
            _ => {
                if size.is_some() {
                    if parts.len() < 3 {
                        return Err(anyhow!(
                            "Invalid data line on line {}: '{}'. Expected 3 float values, found {}",
                            line_num,
                            line,
                            parts.len()
                        ));
                    }
                    let r: f32 = parts[0].parse().map_err(|e| {
                        anyhow!(
                            "Failed to parse R value on line {}: '{}'. Error: {}",
                            line_num,
                            line,
                            e
                        )
                    })?;
                    let g: f32 = parts[1].parse().map_err(|e| {
                        anyhow!(
                            "Failed to parse G value on line {}: '{}'. Error: {}",
                            line_num,
                            line,
                            e
                        )
                    })?;
                    let b: f32 = parts[2].parse().map_err(|e| {
                        anyhow!(
                            "Failed to parse B value on line {}: '{}'. Error: {}",
                            line_num,
                            line,
                            e
                        )
                    })?;
                    data.push(r);
                    data.push(g);
                    data.push(b);
                }
            }
        }
    }

    let lut_size = size.ok_or(anyhow!("LUT_3D_SIZE not found in .cube file"))?;
    let expected_len = (lut_size * lut_size * lut_size * 3) as usize;
    if data.len() != expected_len {
        return Err(anyhow!(
            "LUT data size mismatch. Expected {} float values (for size {}), but found {}. The file may be corrupt or incomplete.",
            expected_len,
            lut_size,
            data.len()
        ));
    }

    Ok(Lut {
        size: lut_size,
        data,
    })
}

fn parse_3dl(reader: impl BufRead) -> anyhow::Result<Lut> {
    let mut data: Vec<f32> = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() == 3 {
            let r: f32 = parts[0].parse()?;
            let g: f32 = parts[1].parse()?;
            let b: f32 = parts[2].parse()?;
            data.push(r);
            data.push(g);
            data.push(b);
        }
    }

    let total_values = data.len();
    if total_values == 0 {
        return Err(anyhow!("No data found in 3DL file"));
    }
    let num_entries = total_values / 3;
    let size = (num_entries as f64).cbrt().round() as u32;

    if size * size * size != num_entries as u32 {
        return Err(anyhow!(
            "Invalid 3DL LUT data size: the number of entries ({}) is not a perfect cube.",
            num_entries
        ));
    }

    Ok(Lut { size, data })
}

fn parse_hald(image: DynamicImage) -> anyhow::Result<Lut> {
    let (width, height) = image.dimensions();
    if width != height {
        return Err(anyhow!(
            "HALD image must be square, but dimensions are {}x{}",
            width,
            height
        ));
    }

    let total_pixels = width * height;
    let size = (total_pixels as f64).cbrt().round() as u32;

    if size * size * size != total_pixels {
        return Err(anyhow!(
            "Invalid HALD image dimensions: total pixels ({}) is not a perfect cube.",
            total_pixels
        ));
    }

    let mut data = Vec::with_capacity((total_pixels * 3) as usize);
    let rgb_image = image.to_rgb8();

    for pixel in rgb_image.pixels() {
        data.push(pixel[0] as f32 / 255.0);
        data.push(pixel[1] as f32 / 255.0);
        data.push(pixel[2] as f32 / 255.0);
    }

    Ok(Lut { size, data })
}

pub fn parse_lut_file(path_str: &str) -> anyhow::Result<Lut> {
    let normalized_path_str = path_str.strip_prefix(r"\\?\").unwrap_or(path_str);

    if normalized_path_str.starts_with(r"\\") || normalized_path_str.starts_with("//") {
        return Err(anyhow!("Network paths (UNC) are not allowed for LUTs"));
    }

    let path = std::path::Path::new(path_str);
    if path
        .components()
        .any(|component| component == std::path::Component::ParentDir)
    {
        return Err(anyhow!("Directory traversal (..) is not allowed"));
    }

    if let Some(std::path::Component::Prefix(prefix)) = path.components().next() {
        match prefix.kind() {
            std::path::Prefix::UNC(_, _)
            | std::path::Prefix::VerbatimUNC(_, _)
            | std::path::Prefix::DeviceNS(_) => {
                return Err(anyhow!("Device/UNC prefix paths are not allowed"));
            }
            _ => {}
        }
    }

    let (extension, bytes): (String, Option<Vec<u8>>) =
        if cfg!(target_os = "android") && is_android_content_uri(path_str) {
            #[cfg(target_os = "android")]
            {
                let resolved_name = resolve_android_content_uri_name(path_str)
                    .unwrap_or_else(|_| path_str.to_string());
                let ext = Path::new(&resolved_name)
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or("cube")
                    .to_lowercase();
                let uri_bytes = read_android_content_uri(path_str).map_err(|e| anyhow!("{}", e))?;
                (ext, Some(uri_bytes))
            }
            #[cfg(not(target_os = "android"))]
            {
                (String::new(), None)
            }
        } else {
            let ext = Path::new(path_str)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            (ext, None)
        };

    match extension.as_str() {
        "cube" => {
            if let Some(b) = bytes {
                parse_cube(BufReader::new(Cursor::new(b)))
            } else {
                let file = File::open(path_str)?;
                parse_cube(BufReader::new(file))
            }
        }
        "3dl" => {
            if let Some(b) = bytes {
                parse_3dl(BufReader::new(Cursor::new(b)))
            } else {
                let file = File::open(path_str)?;
                parse_3dl(BufReader::new(file))
            }
        }
        "png" | "jpg" | "jpeg" | "tiff" => {
            let img = if let Some(b) = bytes {
                image::load_from_memory(&b)?
            } else {
                image::open(path_str)?
            };
            parse_hald(img)
        }
        _ => Err(anyhow!("Unsupported LUT file format: {}", extension)),
    }
}

pub fn generate_identity_lut_image(size: u32) -> DynamicImage {
    let width = size;
    let height = size * size;
    let mut img = Rgb32FImage::new(width, height);

    for z in 0..size {
        for y in 0..size {
            for x in 0..size {
                let r = x as f32 / (size - 1) as f32;
                let g = y as f32 / (size - 1) as f32;
                let b = z as f32 / (size - 1) as f32;

                img.put_pixel(x, z * size + y, Rgb([r, g, b]));
            }
        }
    }

    DynamicImage::ImageRgb32F(img)
}

pub fn convert_image_to_cube_lut(image: &DynamicImage, size: u32) -> Result<Vec<u8>, String> {
    let f32_image = image.to_rgb32f();
    let mut out = String::new();

    out.push_str(&format!("LUT_3D_SIZE {}\n", size));
    out.push_str("DOMAIN_MIN 0.0 0.0 0.0\n");
    out.push_str("DOMAIN_MAX 1.0 1.0 1.0\n");

    for z in 0..size {
        for y in 0..size {
            for x in 0..size {
                let pixel = f32_image.get_pixel(x, z * size + y);
                out.push_str(&format!(
                    "{:.6} {:.6} {:.6}\n",
                    pixel[0].clamp(0.0, 1.0),
                    pixel[1].clamp(0.0, 1.0),
                    pixel[2].clamp(0.0, 1.0)
                ));
            }
        }
    }

    Ok(out.into_bytes())
}

pub fn get_or_load_lut(state: &State<AppState>, path: &str) -> Result<Arc<Lut>, String> {
    get_or_load(&state.lut_cache, path, || {
        parse_lut_file(path).map_err(|e| e.to_string())
    })
}

use std::sync::OnceLock;

/// Cached built-in LUT domain map (file name -> is scene-referred), populated
/// by the first `list_luts` call. Used by `get_all_adjustments_from_json` to
/// correct stale `lutIsSceneReferred` values stored before the manifest existed
/// (the old "built-in implies scene-referred" guess wrote `true` for the sRGB
/// camera-style pack, which renders through the wrong shader slot).
static SCENE_REFERRED_LUTS: OnceLock<std::collections::HashSet<String>> = OnceLock::new();

fn scene_referred_cache() -> &'static std::collections::HashSet<String> {
    SCENE_REFERRED_LUTS.get_or_init(std::collections::HashSet::new)
}

/// Returns `Some(is_scene_referred)` if the path is a known built-in LUT,
/// `None` for user-imported LUTs (their stored flag is trusted as-is).
pub fn lut_domain_hint(path: &str) -> Option<bool> {
    let file_name = path.rsplit(['/', '\\']).next()?;
    if scene_referred_cache().contains(file_name) {
        Some(true)
    } else {
        // If the cache is populated and this is a built-in path but not in the
        // scene list, it is display-referred.
        let populated = !scene_referred_cache().is_empty()
            || SCENE_REFERRED_LUTS.get().is_some_and(|s| !s.is_empty());
        if populated { Some(false) } else { None }
    }
}

fn cache_scene_referred_luts(entries: &[LutEntry]) {
    let scene: std::collections::HashSet<String> = entries
        .iter()
        .filter(|e| e.is_scene_referred)
        .map(|e| {
            e.path
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&e.path)
                .to_string()
        })
        .collect();
    let _ = SCENE_REFERRED_LUTS.set(scene);
}

#[tauri::command]
pub fn list_luts(app_handle: AppHandle) -> Result<Vec<LutEntry>, String> {
    let mut all_luts = Vec::new();

    if let Some(resource_path) = film_luts_dir(&app_handle)
        && let Ok(built_in) = list_luts_in_dir(&resource_path, true)
    {
        cache_scene_referred_luts(&built_in);
        all_luts.extend(built_in);
    }

    let data_dir = crate::app_paths::app_data_dir(&app_handle).map_err(|e| e.to_string())?;
    let luts_dir = get_luts_dir(&data_dir).map_err(|e| e.to_string())?;

    #[cfg(target_os = "android")]
    {
        if let Ok(user_luts) = list_luts_in_dir(&luts_dir, false) {
            all_luts.extend(user_luts);
        }
        if let Ok(cached) = list_luts_in_cache() {
            all_luts.extend(cached);
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        if let Ok(user_luts) = list_luts_in_dir(&luts_dir, false) {
            all_luts.extend(user_luts);
        }
    }

    Ok(all_luts)
}

#[cfg(target_os = "android")]
fn get_lut_cache_dir() -> anyhow::Result<PathBuf> {
    let cache_path = get_android_cached_lut_path("_", "tmp")?;
    cache_path
        .parent()
        .ok_or_else(|| anyhow!("Invalid cache path"))
        .map(|p| p.to_path_buf())
}

#[cfg(target_os = "android")]
fn list_luts_in_cache() -> anyhow::Result<Vec<LutEntry>> {
    let cache_dir = get_lut_cache_dir()?;

    if !cache_dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries: Vec<LutEntry> = Vec::new();
    for entry in read_dir(&cache_dir)? {
        let path = entry?.path();
        let extension = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        let is_supported = matches!(
            extension.as_str(),
            "cube" | "3dl" | "png" | "jpg" | "jpeg" | "tiff"
        );

        if is_supported {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("LUT")
                .to_string();
            entries.push(LutEntry {
                name,
                path: strip_verbatim(&path).to_string_lossy().into_owned(),
                is_built_in: false,
            });
        }
    }
    entries.sort_by_key(|a| a.name.to_lowercase());
    Ok(entries)
}

#[tauri::command]
pub fn import_luts(
    app_handle: AppHandle,
    source_paths: Vec<String>,
) -> Result<Vec<LutEntry>, String> {
    let data_dir = crate::app_paths::app_data_dir(&app_handle).map_err(|e| e.to_string())?;
    let luts_dir = get_luts_dir(&data_dir).map_err(|e| e.to_string())?;
    import_luts_to_dir(&luts_dir, &source_paths).map_err(|e| e.to_string())?;

    list_luts(app_handle)
}

#[tauri::command]
pub fn remove_lut(app_handle: AppHandle, path: String) -> Result<Vec<LutEntry>, String> {
    let data_dir = crate::app_paths::app_data_dir(&app_handle).map_err(|e| e.to_string())?;
    let luts_dir = strip_verbatim(&get_luts_dir(&data_dir).map_err(|e| e.to_string())?);
    let target_path = strip_verbatim(Path::new(&path));

    if let Some(resource_path) = film_luts_dir(&app_handle)
        && target_path.starts_with(&resource_path)
    {
        return Err("Cannot delete built-in film emulations".to_string());
    }

    #[cfg(target_os = "android")]
    {
        let cache_dir = strip_verbatim(&get_lut_cache_dir().map_err(|e| e.to_string())?);
        if !target_path.starts_with(&luts_dir) && !target_path.starts_with(&cache_dir) {
            return Err(
                "Access denied: Cannot remove files outside the user LUT directory".to_string(),
            );
        }
    }
    #[cfg(not(target_os = "android"))]
    if !target_path.starts_with(&luts_dir) {
        return Err(
            "Access denied: Cannot remove files outside the user LUT directory".to_string(),
        );
    }

    if target_path.exists() {
        std::fs::remove_file(&target_path).map_err(|e| e.to_string())?;
    } else {
        return Err("LUT file not found".to_string());
    }

    list_luts(app_handle)
}

fn render_lut_swatch(
    context: &crate::render::gpu::context::GpuContext,
    state: &State<AppState>,
    source: &crate::render::source::RenderSource,
    mask_bitmaps: &[image::GrayImage],
    adjustments: AllAdjustments,
    lut_path: &str,
) -> Option<String> {
    let lut = get_swatch_lut(state, lut_path).ok()?;
    let processed = process_and_get_dynamic_image(
        context,
        state,
        source,
        RenderRequest {
            adjustments,
            mask_bitmaps,
            mask_cache_key: None,
            lut: Some(lut),
            roi: None,
            hsl_probe: false,
            motion_blur: Default::default(),
        },
        "generate_lut_previews",
    )
    .ok()?;

    let rgb = processed.to_rgb8();
    let (width, height) = rgb.dimensions();
    let bytes = Encoder::new(Preset::BaselineFastest)
        .quality(80)
        .encode_rgb(&rgb.into_vec(), width, height)
        .ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        general_purpose::STANDARD.encode(&bytes)
    ))
}

static LUT_PREVIEW_RUN: AtomicU64 = AtomicU64::new(0);

#[tauri::command]
pub async fn generate_lut_previews(
    luts: Vec<LutPreviewRequest>,
    size: u32,
    js_adjustments: serde_json::Value,
    on_preview: Channel<LutPreview>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let run = LUT_PREVIEW_RUN.fetch_add(1, Ordering::SeqCst) + 1;
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let context = crate::render::gpu::context::get_or_init_gpu_context(&state, &app_handle)?;
        let loaded_image = state
            .original_image
            .lock()
            .unwrap()
            .clone()
            .ok_or("No original image loaded for LUT previews")?;
        let is_raw = loaded_image.is_raw;
        let generation = state.load_image_generation.load(Ordering::SeqCst);

        // Swatch base = the caller's current adjustments, so the grid shows the
        // LUT on the user's actual grade (geometry, crop, basic/color tools and
        // masks included) - the same contract as the hover preview override on
        // desktop. Touch platforms have no hover and only ever see these
        // swatches, so they must be representative on their own.
        let mut base_json = if js_adjustments.is_object() {
            js_adjustments
        } else {
            serde_json::json!({})
        };
        // Scrub the LUT state: each swatch re-inserts only the LUT it
        // previews, and the payload should not carry `lutData` blobs around.
        if let Some(object) = base_json.as_object_mut() {
            for key in [
                "lutPath",
                "lutName",
                "lutData",
                "lutSize",
                "lutIsSceneReferred",
            ] {
                object.remove(key);
            }
        }
        // Sidecars without the flag still render from the raw baseline, but an
        // explicit opt-out from the editor must survive.
        crate::render::adjustment_utils::ensure_baseline_exposure(&mut base_json);

        let preview =
            prepare_preview_source(&state, &context, &loaded_image, &base_json, size, None)?;
        let source = preview.render_source(&base_json);
        let (scale_for_gpu, unscaled_crop_offset) = (preview.scale, preview.unscaled_crop_offset);
        let (img_w, img_h) = source.size();

        let mask_definitions: Vec<MaskDefinition> = base_json
            .get("masks")
            .and_then(|masks| serde_json::from_value(masks.clone()).ok())
            .unwrap_or_default();
        let scaled_crop_offset = (
            unscaled_crop_offset.0 * scale_for_gpu,
            unscaled_crop_offset.1 * scale_for_gpu,
        );
        let mask_bitmaps = get_cached_or_generate_masks(
            &state,
            &loaded_image.path,
            &mask_definitions,
            img_w,
            img_h,
            scale_for_gpu,
            scaled_crop_offset,
            &base_json,
        )
        .bitmaps;

        let render = resolve_render_options_from_handle(&app_handle, is_raw);
        // Hover parity: keep the user's intensity slider so a swatch matches
        // the live override preview; callers without LUT state get the full
        // look.
        let lut_intensity = base_json
            .get("lutIntensity")
            .and_then(Value::as_f64)
            .unwrap_or(100.0);

        for request in luts {
            if LUT_PREVIEW_RUN.load(Ordering::SeqCst) != run
                || state.load_image_generation.load(Ordering::SeqCst) != generation
            {
                return Err("LUT previews cancelled".to_string());
            }
            let mut swatch_json = base_json.clone();
            if let Some(object) = swatch_json.as_object_mut() {
                object.insert("lutPath".into(), Value::String("preview".into()));
                object.insert("lutIntensity".into(), serde_json::json!(lut_intensity));
                object.insert(
                    "lutIsSceneReferred".into(),
                    serde_json::json!(request.is_scene_referred),
                );
            }
            let mut swatch_adjustments =
                get_all_adjustments_from_json(&swatch_json, is_raw, loaded_image.as_shot, render);
            swatch_adjustments.global.show_clipping = 0;

            let thumb = render_lut_swatch(
                &context,
                &state,
                &source,
                &mask_bitmaps,
                swatch_adjustments,
                &request.path,
            );
            on_preview
                .send(LutPreview {
                    path: request.path,
                    thumb,
                })
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn load_and_parse_lut(
    path: String,
    app_handle: AppHandle,
) -> Result<LutParseResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let lut = get_or_load_lut(&state, &path)?;
        Ok(LutParseResult { size: lut.size })
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn export_adjustments_as_lut(
    js_adjustments: &Value,
    source_path_str: &str,
    context: &Arc<GpuContext>,
    state: &tauri::State<AppState>,
    app_handle: &tauri::AppHandle,
    cancellation_token: &AtomicBool,
) -> Result<Vec<u8>, String> {
    ensure_export_not_cancelled(cancellation_token)?;
    let lut_size = 33;
    let identity_image = generate_identity_lut_image(lut_size);

    let render = RenderOptions {
        baseline: BaselineOptions::default(),
        ..resolve_render_options_from_handle(app_handle, false)
    };
    let mut all_adjustments =
        get_all_adjustments_from_json(js_adjustments, false, as_shot(source_path_str), render);

    all_adjustments.global.show_clipping = 0;
    all_adjustments.global.vignette_amount = 0.0;
    all_adjustments.global.grain_amount = 0.0;
    all_adjustments.global.grain_softness = 0.0;
    all_adjustments.global.sharpness = 0.0;
    all_adjustments.global.clarity = 0.0;
    all_adjustments.global.dehaze = 0.0;
    all_adjustments.global.structure = 0.0;
    all_adjustments.global.centré = 0.0;
    all_adjustments.global.glow_amount = 0.0;
    all_adjustments.global.halation_amount = 0.0;
    all_adjustments.global.flare_amount = 0.0;
    all_adjustments.global.luma_noise_reduction = 0.0;
    all_adjustments.global.color_noise_reduction = 0.0;
    all_adjustments.global.chromatic_aberration_red_cyan = 0.0;
    all_adjustments.global.chromatic_aberration_blue_yellow = 0.0;

    let lut_path = js_adjustments["lutPath"].as_str();
    let lut = lut_path.and_then(|p| get_or_load_lut(state, p).ok());
    let processed_lut = process_and_get_dynamic_image(
        context,
        state,
        &RenderSource::plain(&identity_image),
        RenderRequest {
            adjustments: all_adjustments,
            mask_bitmaps: &[],
            mask_cache_key: None,
            lut,
            roi: None,
            hsl_probe: false,
            motion_blur: Default::default(),
        },
        "export_lut",
    )?;
    ensure_export_not_cancelled(cancellation_token)?;

    let cube_lut = convert_image_to_cube_lut(&processed_lut, lut_size)?;
    ensure_export_not_cancelled(cancellation_token)?;
    Ok(cube_lut)
}

#[cfg(test)]
mod builtin_pack_tests {
    //! Whatever ships in `resources/film_luts` must be listable and parseable
    //! by the production parser (plain 3D cubes, DOMAIN 0..1, no 1D shaper).
    //!
    //! The CamRAW pack is a local, non-redistributable test asset (gitignored),
    //! so this test only asserts what is actually present.

    use super::*;

    #[test]
    fn builtin_pack_parses() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/film_luts");
        let entries = list_luts_in_dir(&dir, true).expect("resource dir must list");

        let mut grids: std::collections::BTreeMap<u32, usize> = Default::default();
        for entry in &entries {
            let lut = parse_lut_file(&entry.path).unwrap_or_else(|e| panic!("{}: {e}", entry.name));
            assert!(
                matches!(lut.size, 17 | 25 | 33 | 65),
                "{}: unexpected grid {}",
                entry.name,
                lut.size
            );
            *grids.entry(lut.size).or_default() += 1;
        }

        println!("{} LUTs, grids {:?}", entries.len(), grids);
        assert!(
            entries.len() >= 6,
            "the bundled spektrafilm set must be present"
        );
    }
}
