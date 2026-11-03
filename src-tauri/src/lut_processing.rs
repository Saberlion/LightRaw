use crate::android_integration::is_android_content_uri;
#[cfg(target_os = "android")]
use crate::android_integration::{
    get_android_cached_lut_path, read_android_content_uri, resolve_android_content_uri_name,
};
use anyhow::anyhow;
use image::{DynamicImage, GenericImageView, Rgb, Rgb32FImage};
use serde::{Deserialize, Serialize};
use std::fs::{File, copy, create_dir_all, read_dir};
use std::io::{BufRead, BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::{Engine as _, engine::general_purpose};
use mozjpeg_rs::{Encoder, Preset};
use tauri::{AppHandle, Manager, State};

#[cfg(any(target_os = "android", target_env = "ohos"))]
use include_dir::{Dir, include_dir};

/// Built-in LUTs are bundled as Tauri resources on desktop, but the mobile
/// platforms cannot resolve `BaseDirectory::Resource` (on OHOS the tauri fork's
/// resource resolver returns `UnsupportedPlatform`, and the HAP does not ship
/// `src-tauri/resources` at all). Embed them in the binary and unpack once into
/// app_data_dir, mirroring lens_correction.rs.
#[cfg(any(target_os = "android", target_env = "ohos"))]
static BUILTIN_LUTS: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/resources/film_luts");

use crate::AppState;
use crate::cache_utils::calculate_transform_hash;
use crate::image_processing::{
    RenderRequest, get_all_adjustments_from_json, mark_baseline_exposure,
    process_and_get_dynamic_image, resolve_render_options_from_handle,
};

#[derive(Debug, Clone)]
pub struct Lut {
    pub size: u32,
    pub data: Vec<f32>,
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

    if let Ok(existing) = std::fs::read_to_string(&marker) {
        if existing == stamp {
            return Some(dir);
        }
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
            if let Some(name_str) = name.to_str() {
                if !embedded.contains(name_str) && name_str != ".stamp" {
                    let _ = std::fs::remove_file(entry.path());
                }
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

fn unique_lut_destination(dir: &Path, stem: &str, extension: &str) -> PathBuf {
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
        let destination = unique_lut_destination(dir, stem, &extension);
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
    let destination = unique_lut_destination(&cache_dir, &stem, &extension);
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

    if path_str.contains("..") {
        return Err(anyhow!("Directory traversal (..) is not allowed"));
    }

    let path = std::path::Path::new(path_str);
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
    let mut cache = state.lut_cache.lock().unwrap();
    if let Some(lut) = cache.get(path) {
        return Ok(lut.clone());
    }

    let lut = parse_lut_file(path).map_err(|e| e.to_string())?;
    let arc_lut = Arc::new(lut);
    cache.insert(path.to_string(), arc_lut.clone());
    Ok(arc_lut)
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
    context: &crate::image_processing::GpuContext,
    state: &State<AppState>,
    base_image: &DynamicImage,
    transform_hash: u64,
    adjustments: crate::image_processing::AllAdjustments,
    lut_path: &str,
) -> Option<String> {
    let lut = get_or_load_lut(state, lut_path).ok()?;
    let processed = process_and_get_dynamic_image(
        context,
        state,
        base_image,
        transform_hash,
        RenderRequest {
            adjustments,
            mask_bitmaps: &[],
            mask_cache_key: None,
            lut: Some(lut),
            roi: None,
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

#[tauri::command]
pub async fn generate_lut_previews(
    luts: Vec<LutPreviewRequest>,
    size: u32,
    app_handle: AppHandle,
) -> Result<Vec<LutPreview>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let context = crate::image_processing::get_or_init_gpu_context(&state, &app_handle)?;
        let loaded_image = state
            .original_image
            .lock()
            .unwrap()
            .clone()
            .ok_or("No original image loaded for LUT previews")?;
        let is_raw = loaded_image.is_raw;

        let base_json = serde_json::json!({});
        let (base_image, _scale, _offset) =
            crate::generate_transformed_preview(&state, &loaded_image, &base_json, size)?;

        let render = resolve_render_options_from_handle(&app_handle, is_raw);
        let transform_hash = calculate_transform_hash(&base_json);

        let previews = luts
            .into_iter()
            .map(|request| {
                let mut swatch_lut_json = serde_json::json!({
                    "lutPath": "preview",
                    "lutIntensity": 100,
                    "lutIsSceneReferred": request.is_scene_referred,
                    "sectionVisibility": { "effects": true }
                });
                mark_baseline_exposure(&mut swatch_lut_json);
                let swatch_adjustments = get_all_adjustments_from_json(
                    &swatch_lut_json,
                    is_raw,
                    loaded_image.as_shot,
                    render,
                );

                let thumb = render_lut_swatch(
                    &context,
                    &state,
                    &base_image,
                    transform_hash,
                    swatch_adjustments,
                    &request.path,
                );
                LutPreview {
                    path: request.path,
                    thumb,
                }
            })
            .collect();

        Ok(previews)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn load_and_parse_lut(path: String, state: State<AppState>) -> Result<LutParseResult, String> {
    let lut = parse_lut_file(&path).map_err(|e| e.to_string())?;
    let lut_size = lut.size;

    let mut cache = state.lut_cache.lock().unwrap();
    cache.insert(path, Arc::new(lut));

    Ok(LutParseResult { size: lut_size })
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
