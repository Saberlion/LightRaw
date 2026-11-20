use crate::app::state::AppState;
use crate::library::file_ops::write_file_atomic;
use crate::library::preset_converter;
use crate::masks::generation::{MaskDefinition, get_cached_or_generate_masks};
#[cfg(target_os = "android")]
use crate::platform::android::{
    is_android_content_uri, read_android_content_uri, resolve_android_content_uri_name,
};
use crate::render::gpu::context::get_or_init_gpu_context;
use crate::render::gpu::output::process_and_get_dynamic_image;
use crate::render::gpu::processor::RenderRequest;
use crate::render::options::resolve_render_options_from_handle;
use crate::render::preview::prepare_preview_source;
use crate::render::uniforms::get_all_adjustments_from_json;
use anyhow::Result;
use image::codecs::jpeg::JpegEncoder;
use regex::regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use tauri::{AppHandle, Manager, ipc::Response};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub adjustments: Value,
    #[serde(rename = "includeMasks", skip_serializing_if = "Option::is_none")]
    pub include_masks: Option<bool>,
    #[serde(
        rename = "includeCropTransform",
        skip_serializing_if = "Option::is_none"
    )]
    pub include_crop_transform: Option<bool>,
    #[serde(rename = "presetType", skip_serializing_if = "Option::is_none")]
    pub preset_type: Option<String>,
}

#[derive(Serialize)]
struct ExportPresetFile<'a> {
    creator: &'a str,
    presets: &'a [PresetItem],
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PresetFolder {
    pub id: String,
    pub name: String,
    pub children: Vec<Preset>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub enum PresetItem {
    Preset(Preset),
    Folder(PresetFolder),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PresetFile {
    pub presets: Vec<PresetItem>,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PresetImportFailure {
    pub file_name: String,
    pub error: String,
}

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PresetImportResult {
    pub presets: Vec<PresetItem>,
    pub failures: Vec<PresetImportFailure>,
}

#[tauri::command]
pub async fn generate_preset_preview(
    js_adjustments: serde_json::Value,
    app_handle: tauri::AppHandle,
) -> Result<Response, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let context = get_or_init_gpu_context(&state, &app_handle)?;

        let loaded_image = state
            .original_image
            .lock()
            .unwrap()
            .clone()
            .ok_or("No original image loaded for preset preview")?;
        let is_raw = loaded_image.is_raw;

        let mut js_adjustments = js_adjustments;
        if crate::ai::jobs::has_ai_results(&js_adjustments) {
            let (source_path, sidecar_path) =
                crate::library::sidecars::parse_virtual_path(&loaded_image.path);
            if let Some(source_id) = crate::ai::jobs::source_id(&source_path) {
                let existing = crate::metadata::sidecar::load_sidecar(&sidecar_path).adjustments;
                crate::ai::jobs::reconcile_ai_results(
                    &mut js_adjustments,
                    &existing,
                    &source_id,
                    false,
                );
            }
        }

        const PRESET_PREVIEW_DIM: u32 = 400;

        let preview = prepare_preview_source(
            &state,
            &context,
            &loaded_image,
            &js_adjustments,
            PRESET_PREVIEW_DIM,
        )?;
        let source = preview.render_source(&js_adjustments);
        let (scale_for_gpu, unscaled_crop_offset) = (preview.scale, preview.unscaled_crop_offset);
        let (img_w, img_h) = source.size();

        let mask_definitions: Vec<MaskDefinition> = js_adjustments
            .get("masks")
            .and_then(|m| serde_json::from_value(m.clone()).ok())
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
            &js_adjustments,
        )
        .bitmaps;

        let render = resolve_render_options_from_handle(&app_handle, is_raw);
        let mut all_adjustments =
            get_all_adjustments_from_json(&js_adjustments, is_raw, loaded_image.as_shot, render);
        all_adjustments.global.show_clipping = 0;
        let lut_path = js_adjustments["lutPath"].as_str();
        let lut = lut_path
            .and_then(|p| crate::adjustments::lut_processing::get_or_load_lut(&state, p).ok());

        let processed_image = process_and_get_dynamic_image(
            &context,
            &state,
            &source,
            RenderRequest {
                adjustments: all_adjustments,
                mask_bitmaps: &mask_bitmaps,
                mask_cache_key: None,
                lut,
                roi: None,
                hsl_probe: false,
                motion_blur: crate::effects::motion_blur::MotionBlurs::new(
                    &loaded_image.path,
                    &js_adjustments,
                    (img_w, img_h),
                    scaled_crop_offset,
                    loaded_image.image.width().max(loaded_image.image.height()) as f32
                        * scale_for_gpu,
                ),
            },
            "generate_preset_preview",
        )?;

        let mut buf = Cursor::new(Vec::new());
        processed_image
            .to_rgb8()
            .write_with_encoder(JpegEncoder::new_with_quality(&mut buf, 80))
            .map_err(|e| e.to_string())?;

        Ok(Response::new(buf.into_inner()))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn get_presets_path(app_handle: &AppHandle) -> Result<std::path::PathBuf, String> {
    let presets_dir = crate::app_paths::app_data_dir(app_handle)
        .map_err(|e| e.to_string())?
        .join("presets");

    if !presets_dir.exists() {
        fs::create_dir_all(&presets_dir).map_err(|e| e.to_string())?;
    }

    Ok(presets_dir.join("presets.json"))
}

#[tauri::command]
pub fn load_presets(app_handle: AppHandle) -> Result<Vec<PresetItem>, String> {
    let path = get_presets_path(&app_handle)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_presets(presets: Vec<PresetItem>, app_handle: AppHandle) -> Result<(), String> {
    let path = get_presets_path(&app_handle)?;
    let json_string = serde_json::to_string_pretty(&presets).map_err(|e| e.to_string())?;
    write_file_atomic(&path, json_string.as_bytes()).map_err(|e| e.to_string())?;
    crate::adjustments::develop_defaults::refresh(&app_handle);
    Ok(())
}

fn preset_file_display_name(file_path: &str) -> String {
    #[cfg(target_os = "android")]
    {
        if is_android_content_uri(file_path) {
            return resolve_android_content_uri_name(file_path)
                .unwrap_or_else(|_| file_path.to_string());
        }
    }

    Path::new(file_path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| file_path.to_string())
}

fn collect_top_level_preset_names(items: &[PresetItem]) -> HashSet<String> {
    items
        .iter()
        .map(|item| match item {
            PresetItem::Preset(p) => p.name.clone(),
            PresetItem::Folder(f) => f.name.clone(),
        })
        .collect()
}

fn read_preset_file_to_string(file_path: &str) -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        if is_android_content_uri(file_path) {
            let bytes = read_android_content_uri(file_path)?;
            return String::from_utf8(bytes)
                .map_err(|e| format!("Preset file is not valid UTF-8: {}", e));
        }
    }

    fs::read_to_string(file_path).map_err(|e| e.to_string())
}

fn parse_preset_file(file_path: &str) -> Result<Vec<PresetItem>, String> {
    let lower_path = preset_file_display_name(file_path).to_lowercase();
    let content = read_preset_file_to_string(file_path)
        .map_err(|e| format!("Failed to read preset file: {}", e))?;

    let is_legacy = if lower_path.ends_with(".xmp") || lower_path.ends_with(".lrtemplate") {
        true
    } else if lower_path.ends_with(".rrpreset") {
        false
    } else {
        !content.trim_start().starts_with('{')
    };

    if !is_legacy {
        let preset_file: PresetFile = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse preset file: {}", e))?;
        return Ok(preset_file.presets);
    }

    let xmp_content = if lower_path.ends_with(".lrtemplate") {
        if let Some(caps) = regex!(r#"(?s)s.xmp = "(.*)""#).captures(&content) {
            caps.get(1)
                .map(|m| m.as_str().replace(r#"\""#, r#"""#))
                .unwrap_or(content)
        } else {
            content
        }
    } else {
        content
    };

    let converted_preset = preset_converter::convert_xmp_to_preset(&xmp_content)?;
    Ok(vec![PresetItem::Preset(converted_preset)])
}

fn merge_imported_items(
    target: &mut Vec<PresetItem>,
    taken_names: &mut HashSet<String>,
    imported: Vec<PresetItem>,
) {
    for mut imported_item in imported {
        let original_name = match &mut imported_item {
            PresetItem::Preset(p) => {
                p.id = Uuid::new_v4().to_string();
                p.name.clone()
            }
            PresetItem::Folder(f) => {
                f.id = Uuid::new_v4().to_string();
                for child in &mut f.children {
                    child.id = Uuid::new_v4().to_string();
                }
                f.name.clone()
            }
        };

        let mut new_name = original_name.clone();
        let mut counter = 1;
        while taken_names.contains(&new_name) {
            new_name = format!("{} ({})", original_name, counter);
            counter += 1;
        }

        match &mut imported_item {
            PresetItem::Preset(p) => p.name = new_name.clone(),
            PresetItem::Folder(f) => f.name = new_name.clone(),
        }

        taken_names.insert(new_name);
        target.push(imported_item);
    }
}

fn import_preset_file_into_library(
    file_path: &str,
    app_handle: AppHandle,
) -> Result<Vec<PresetItem>, String> {
    let imported = parse_preset_file(file_path)?;

    let mut current_presets = load_presets(app_handle.clone())?;
    let mut taken_names = collect_top_level_preset_names(&current_presets);
    merge_imported_items(&mut current_presets, &mut taken_names, imported);

    save_presets(current_presets.clone(), app_handle)?;
    Ok(current_presets)
}

#[tauri::command]
pub fn handle_import_presets_from_file(
    file_path: String,
    app_handle: AppHandle,
) -> Result<Vec<PresetItem>, String> {
    import_preset_file_into_library(&file_path, app_handle)
}

#[tauri::command]
pub fn handle_import_legacy_presets_from_file(
    file_path: String,
    app_handle: AppHandle,
) -> Result<Vec<PresetItem>, String> {
    import_preset_file_into_library(&file_path, app_handle)
}

#[tauri::command]
pub fn handle_import_presets_from_files(
    file_paths: Vec<String>,
    app_handle: AppHandle,
) -> Result<PresetImportResult, String> {
    let mut current_presets = load_presets(app_handle.clone())?;
    let mut taken_names = collect_top_level_preset_names(&current_presets);

    let mut failures: Vec<PresetImportFailure> = Vec::new();
    let mut library_changed = false;

    for file_path in &file_paths {
        match parse_preset_file(file_path) {
            Ok(imported) => {
                library_changed |= !imported.is_empty();
                merge_imported_items(&mut current_presets, &mut taken_names, imported);
            }
            Err(error) => failures.push(PresetImportFailure {
                file_name: preset_file_display_name(file_path),
                error,
            }),
        }
    }

    if library_changed {
        save_presets(current_presets.clone(), app_handle)?;
    }

    Ok(PresetImportResult {
        presets: current_presets,
        failures,
    })
}

#[tauri::command]
pub fn handle_export_presets_to_file(
    presets_to_export: Vec<PresetItem>,
    file_path: String,
) -> Result<(), String> {
    let preset_file = ExportPresetFile {
        creator: "Anonymous",
        presets: &presets_to_export,
    };

    let json_string = serde_json::to_string_pretty(&preset_file)
        .map_err(|e| format!("Failed to serialize presets: {}", e))?;
    fs::write(file_path, json_string).map_err(|e| format!("Failed to write preset file: {}", e))
}
