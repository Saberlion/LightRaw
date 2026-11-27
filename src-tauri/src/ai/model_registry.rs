//! AI model registry — the single source of truth for every model asset
//! the app can load at inference time, plus the Settings model-management
//! view built on top of it (docs/AI_MODEL_MANAGEMENT_DESIGN.md).
//!
//! Every download pin (URL + SHA256) lives here; `ai/models.rs` builds the
//! runtime sessions from these assets. The registry is also the only
//! feature → asset mapping (`mask_model_assets`), so the Settings list and
//! the lazy-download path cannot drift apart.
//!
//! The pin block below was moved verbatim from `ai/models.rs` (2026-11-27).
//! Do not retype hashes by hand: any change to a URL/SHA pin here falls
//! under the AI golden-test rule (AGENTS.md) and must be re-verified on
//! host x86 before landing.

use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;
use tauri::AppHandle;

use crate::ai::models::AiMaskModel;

// ---------------------------------------------------------------------------
// Pinned download assets
// ---------------------------------------------------------------------------

const ENCODER_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/sam_vit_b_01ec64_encoder.onnx?download=true";

const DECODER_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/sam_vit_b_01ec64_decoder.onnx?download=true";

pub(crate) const ENCODER_FILENAME: &str = "sam_vit_b_01ec64_encoder.onnx";

pub(crate) const DECODER_FILENAME: &str = "sam_vit_b_01ec64_decoder.onnx";

const ENCODER_SHA256: &str = "16ab73d9c824886f0de2938c19df22fb9ec3deebfd0de58e65177e479213d7d1";

const DECODER_SHA256: &str = "85d0d672cf5b7fe763edcde429e5533e62f674af4b15c7d688b7673b0ef00bf7";

const BIREFNET_FP16_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/birefnet_general_lite_fp16.onnx?download=true";

pub(crate) const BIREFNET_FP16_FILENAME: &str = "birefnet_general_lite_fp16.onnx";

const BIREFNET_FP16_SHA256: &str =
    "3b72cb3c2138605ced0a4b271e8d13d3b3ca2483cf1665b098b6851767420fa1";

const BIREFNET_FP32_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/birefnet_general_lite.onnx?download=true";

pub(crate) const BIREFNET_FP32_FILENAME: &str = "birefnet_general_lite.onnx";

const BIREFNET_FP32_SHA256: &str =
    "5600024376f572a557870a5eb0afb1e5961636bef4e1e22132025467d0f03333";

const ESAM3_IMAGE_ENCODER_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/efficientsam3_ev_m_image_encoder.onnx?download=true";

pub(crate) const ESAM3_IMAGE_ENCODER_FILENAME: &str = "efficientsam3_ev_m_image_encoder.onnx";

const ESAM3_IMAGE_ENCODER_SHA256: &str =
    "2ed08306579138120dbf9f1110fd31784d1c20fe1b67fa2187a0a2a6d7c82729";

const ESAM3_DECODER_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/efficientsam3_ev_m_decoder.onnx?download=true";

pub(crate) const ESAM3_DECODER_FILENAME: &str = "efficientsam3_ev_m_decoder.onnx";

const ESAM3_DECODER_SHA256: &str =
    "16accc22a07ee99c73ef8a14e286ec567f94c116e0d8efa1a3a6c0bc9a2596a8";

const ESAM3_TEXT_ENCODER_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/efficientsam3_ev_m_text_encoder.onnx?download=true";

pub(crate) const ESAM3_TEXT_ENCODER_FILENAME: &str = "efficientsam3_ev_m_text_encoder.onnx";

const ESAM3_TEXT_ENCODER_SHA256: &str =
    "3065ea6116f21f6e22bc70a0c041e5da9ae05fdec7b48df953394f5657609590";

const ESAM3_SKY_PROMPT_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/efficientsam3_sky_prompt.bin?download=true";

pub(crate) const ESAM3_SKY_PROMPT_FILENAME: &str = "efficientsam3_sky_prompt.bin";

const ESAM3_SKY_PROMPT_SHA256: &str =
    "b8dc43661db640f2e3881466252d154906d90896eb3ed9dbc747bec87a97d180";

const ESAM3_FACE_PROMPT_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/efficientsam3_face_prompt.bin?download=true";

pub(crate) const ESAM3_FACE_PROMPT_FILENAME: &str = "efficientsam3_face_prompt.bin";

const ESAM3_FACE_PROMPT_SHA256: &str =
    "8fdbe63ef853756b1da9cdc642c271a6e422452f3feaf35f429e6aea3a1ffb9";

const FACE_LANDMARKS_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/mediapipe_face_landmarks.onnx?download=true";

pub(crate) const FACE_LANDMARKS_FILENAME: &str = "mediapipe_face_landmarks.onnx";

const FACE_LANDMARKS_SHA256: &str =
    "6524d596b4d9151b0d8d590844566261f21de5186ac38125bdcd1de6974b0bb4";

const FACE_SEGMENTER_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/mediapipe_selfie_multiclass.onnx?download=true";

pub(crate) const FACE_SEGMENTER_FILENAME: &str = "mediapipe_selfie_multiclass.onnx";

const FACE_SEGMENTER_SHA256: &str =
    "a198470a4edec350a33b8182c149e417effc5e61c0506b947f226e7a81b9a08e";

const CLIP_MODEL_URL: &str =
    "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/clip_model.onnx?download=true";

pub(crate) const CLIP_MODEL_FILENAME: &str = "clip_model.onnx";

const CLIP_TOKENIZER_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/clip_tokenizer.json?download=true";

pub(crate) const CLIP_TOKENIZER_FILENAME: &str = "clip_tokenizer.json";

const CLIP_MODEL_SHA256: &str = "57879bb1c23cdeb350d23569dd251ed4b740a96d747c529e94a2bb8040ac5d00";

const DENOISE_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/nind_denoise_utnet_684.onnx?download=true";

pub(crate) const DENOISE_FILENAME: &str = "nind_denoise_utnet_684.onnx";

const DENOISE_SHA256: &str = "ee3586279d514df557ff3f7dec6df37fafc51ba5d3a3435b2cc9ac2d9017e7fe";

// NPU denoise models (OpenHarmony only). The .ms build comes from
// scripts/convert-nind-ms.ps1 (ONNX -> Pad surgery -> converter_lite 2.7.0);
// full recipe + pinned hashes: docs/AI_DENOISE_HW_ACCELERATION.md section 5.6.
#[cfg(target_env = "ohos")]
const DENOISE_MS_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/nind_denoise_utnet_684.ms?download=true";
#[cfg(target_env = "ohos")]
pub(crate) const DENOISE_MS_FILENAME: &str = "nind_denoise_utnet_684.ms";
#[cfg(target_env = "ohos")]
pub(crate) const DENOISE_MS_SHA256: &str = "05d7580e5a6e3c18bd286705f7524090cd69abe4b78067c2a173b322ef151b3b";

// CANN (.om) denoise build, sideloaded; same nind lineage after Pad→Slice
// surgery -> omg (504 is the fixed tile size all TileParams variants use).
// NCHW I/O (no NHWC conversion).
#[cfg(target_env = "ohos")]
pub const DENOISE_OM_FILENAME: &str = "nind_v2.om";

/// CANN (.om) LaMa inpainting build, sideloaded per SoC: fp32 weights
/// (the fp16-weight variant fails NPU runtime Init — partition overflow —
/// docs/LAMA_CANN_NPU_ANALYSIS.md §4).
#[cfg(target_env = "ohos")]
pub const LAMA_OM_FILENAME: &str = "lama_fp32w.om";

/// Side-loaded CANN `.om` for the RapidRes upscale model (same provisioning
/// channel as `nind_v2.om`: never bundled, always SoC-specific, built with
/// `omg --platform=<soc>` per docs/RAPIDRES_NPU_FEASIBILITY.md).
#[cfg(target_env = "ohos")]
pub const UPSCALE_OM_FILENAME: &str = "2xBHI_small_span_rapidraw.om";

const UPSCALE_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/2xBHI_small_span_rapidraw.onnx?download=true";

pub(crate) const UPSCALE_FILENAME: &str = "2xBHI_small_span_rapidraw.onnx";

const UPSCALE_SHA256: &str = "9d3a51deab2518cde257048180070b078840f2aa32326b8bf0e86f15f2a689a0";

const LAMA_URL: &str =
    "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/lama_fp16.onnx?download=true";

pub(crate) const LAMA_FILENAME: &str = "lama_fp16.onnx";

const LAMA_SHA256: &str = "2d6be6277c400d6f1b91819737f7c3da935e5c63d1b521d393be1196a2bfa82c";

const DEPTH_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/depth_anything_v2_vits.onnx?download=true";

pub(crate) const DEPTH_FILENAME: &str = "depth_anything_v2_vits.onnx";

const DEPTH_SHA256: &str = "d2b11a11c1d4a12b47608fa65a17ee9a4c605b55ee1730c8e3b526304f2562be";

const NORMAL_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/moge-2-vits-normal-onnx.onnx?download=true";

pub(crate) const NORMAL_FILENAME: &str = "moge-2-vits-normal-onnx.onnx";

const NORMAL_SHA256: &str = "24eacb5dc7a2c54c7bc98f7de085ffbed79ad006ea5b664c2c2cdc02ff3a52f0";

pub(crate) struct ModelAsset {
    pub(crate) filename: &'static str,
    pub(crate) url: &'static str,
    pub(crate) sha256: Option<&'static str>,
    pub(crate) name: &'static str,
}

pub(crate) const SAM_ENCODER_ASSET: ModelAsset = ModelAsset {
    filename: ENCODER_FILENAME,
    url: ENCODER_URL,
    sha256: Some(ENCODER_SHA256),
    name: "SAM Encoder",
};

pub(crate) const SAM_DECODER_ASSET: ModelAsset = ModelAsset {
    filename: DECODER_FILENAME,
    url: DECODER_URL,
    sha256: Some(DECODER_SHA256),
    name: "SAM Decoder",
};

pub(crate) const BIREFNET_FP16_ASSET: ModelAsset = ModelAsset {
    filename: BIREFNET_FP16_FILENAME,
    url: BIREFNET_FP16_URL,
    sha256: Some(BIREFNET_FP16_SHA256),
    name: "Foreground Model (fp16)",
};

pub(crate) const BIREFNET_FP32_ASSET: ModelAsset = ModelAsset {
    filename: BIREFNET_FP32_FILENAME,
    url: BIREFNET_FP32_URL,
    sha256: Some(BIREFNET_FP32_SHA256),
    name: "Foreground Model (fp32)",
};

pub(crate) const ESAM3_IMAGE_ENCODER_ASSET: ModelAsset = ModelAsset {
    filename: ESAM3_IMAGE_ENCODER_FILENAME,
    url: ESAM3_IMAGE_ENCODER_URL,
    sha256: Some(ESAM3_IMAGE_ENCODER_SHA256),
    name: "EfficientSAM3 Encoder",
};

pub(crate) const ESAM3_DECODER_ASSET: ModelAsset = ModelAsset {
    filename: ESAM3_DECODER_FILENAME,
    url: ESAM3_DECODER_URL,
    sha256: Some(ESAM3_DECODER_SHA256),
    name: "EfficientSAM3 Decoder",
};

pub(crate) const ESAM3_TEXT_ENCODER_ASSET: ModelAsset = ModelAsset {
    filename: ESAM3_TEXT_ENCODER_FILENAME,
    url: ESAM3_TEXT_ENCODER_URL,
    sha256: Some(ESAM3_TEXT_ENCODER_SHA256),
    name: "EfficientSAM3 Text Encoder",
};

pub(crate) const ESAM3_SKY_PROMPT_ASSET: ModelAsset = ModelAsset {
    filename: ESAM3_SKY_PROMPT_FILENAME,
    url: ESAM3_SKY_PROMPT_URL,
    sha256: Some(ESAM3_SKY_PROMPT_SHA256),
    name: "Sky Prompt",
};

pub(crate) const ESAM3_FACE_PROMPT_ASSET: ModelAsset = ModelAsset {
    filename: ESAM3_FACE_PROMPT_FILENAME,
    url: ESAM3_FACE_PROMPT_URL,
    sha256: Some(ESAM3_FACE_PROMPT_SHA256),
    name: "Face Prompt",
};

pub(crate) const FACE_LANDMARKS_ASSET: ModelAsset = ModelAsset {
    filename: FACE_LANDMARKS_FILENAME,
    url: FACE_LANDMARKS_URL,
    sha256: Some(FACE_LANDMARKS_SHA256),
    name: "Face Landmarks Model",
};

pub(crate) const FACE_SEGMENTER_ASSET: ModelAsset = ModelAsset {
    filename: FACE_SEGMENTER_FILENAME,
    url: FACE_SEGMENTER_URL,
    sha256: Some(FACE_SEGMENTER_SHA256),
    name: "Face Segmentation Model",
};

pub(crate) const DEPTH_ASSET: ModelAsset = ModelAsset {
    filename: DEPTH_FILENAME,
    url: DEPTH_URL,
    sha256: Some(DEPTH_SHA256),
    name: "Depth Model",
};

pub(crate) const CLIP_MODEL_ASSET: ModelAsset = ModelAsset {
    filename: CLIP_MODEL_FILENAME,
    url: CLIP_MODEL_URL,
    sha256: Some(CLIP_MODEL_SHA256),
    name: "CLIP Model",
};

pub(crate) const CLIP_TOKENIZER_ASSET: ModelAsset = ModelAsset {
    filename: CLIP_TOKENIZER_FILENAME,
    url: CLIP_TOKENIZER_URL,
    sha256: None,
    name: "CLIP Tokenizer",
};

pub(crate) const DENOISE_ASSET: ModelAsset = ModelAsset {
    filename: DENOISE_FILENAME,
    url: DENOISE_URL,
    sha256: Some(DENOISE_SHA256),
    name: "NIND Denoise Model",
};

#[cfg(target_env = "ohos")]
pub(crate) const DENOISE_MS_ASSET: ModelAsset = ModelAsset {
    filename: DENOISE_MS_FILENAME,
    url: DENOISE_MS_URL,
    sha256: Some(DENOISE_MS_SHA256),
    name: "NIND Denoise (.ms)",
};

pub(crate) const UPSCALE_ASSET: ModelAsset = ModelAsset {
    filename: UPSCALE_FILENAME,
    url: UPSCALE_URL,
    sha256: Some(UPSCALE_SHA256),
    name: "Super Resolution Model",
};

pub(crate) const LAMA_ASSET: ModelAsset = ModelAsset {
    filename: LAMA_FILENAME,
    url: LAMA_URL,
    sha256: Some(LAMA_SHA256),
    name: "Inpainting Model",
};

pub(crate) const NORMAL_ASSET: ModelAsset = ModelAsset {
    filename: NORMAL_FILENAME,
    url: NORMAL_URL,
    sha256: Some(NORMAL_SHA256),
    name: "Normal Model",
};

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

/// Feature grouping used by the Settings model-management view. The enum
/// string is serialized to the frontend (camelCase) and mapped to
/// `settings.aiModels.feature.*` i18n keys there.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum AiFeature {
    SubjectMask,
    ForegroundMask,
    /// Components shared by the Sky/Text/Face mask family.
    SharedMask,
    DepthMask,
    Denoise,
    Upscale,
    Inpaint,
    Tagging,
    Normal,
}

/// Settings view state of one model file. `Bundled` means the model is only
/// present in the OHOS release HAP rawfile bundle and has not been extracted
/// to the app-data models dir yet (ORT loads it from memory).
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum AiModelStatus {
    NotInstalled,
    Ready,
    Corrupt,
    Bundled,
}

/// Settings view row (IPC payload of `list_ai_models`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub filename: &'static str,
    pub feature: AiFeature,
    pub status: AiModelStatus,
    /// On-disk size; for `Bundled` entries the rawfile size (reference only).
    pub size_bytes: u64,
    /// Present in the HAP rawfile bundle → the Settings view files it under
    /// the built-in section (desktop and debug HAPs: always false).
    pub bundled: bool,
    /// CANN/NNRT artifacts (`.om`/`.ms`) need a filesystem path, so they are
    /// extracted to the models dir on first use even when bundled.
    pub disk_required: bool,
    /// A pinned download URL exists (side-loaded `.om` files: false).
    pub downloadable: bool,
    pub sha256: Option<&'static str>,
}

/// BiRefNet ships two weight variants; exactly one is active depending on
/// whether GPU inference is enabled.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum GpuVariant {
    Fp16,
    Fp32,
}

pub(crate) struct RegistryEntry {
    pub id: &'static str,
    /// Settings display name (distinct from `ModelAsset::name`, which also
    /// keys the download-progress events).
    pub name: &'static str,
    /// `None` for side-loaded CANN `.om` artifacts without a download URL.
    pub asset: Option<&'static ModelAsset>,
    pub filename: &'static str,
    pub feature: AiFeature,
    /// Mask features that lazily download this asset via `mask_model_assets`.
    pub mask_models: &'static [AiMaskModel],
    pub gpu_variant: Option<GpuVariant>,
    pub disk_required: bool,
}

/// The full registry, in stable UI order (grouped by feature).
///
/// OHOS-only entries are `cfg`-gated array elements: desktop and OHOS debug
/// builds simply do not see the `.ms`/`.om` rows, mirroring `has_bundled_model`
/// being a runtime no-op there.
pub(crate) fn registry() -> &'static [RegistryEntry] {
    &[
        // --- subject masking (SAM) ---
        RegistryEntry {
            id: "sam_encoder",
            name: "SAM Encoder",
            asset: Some(&SAM_ENCODER_ASSET),
            filename: ENCODER_FILENAME,
            feature: AiFeature::SubjectMask,
            mask_models: &[AiMaskModel::Subject],
            gpu_variant: None,
            disk_required: false,
        },
        RegistryEntry {
            id: "sam_decoder",
            name: "SAM Decoder",
            asset: Some(&SAM_DECODER_ASSET),
            filename: DECODER_FILENAME,
            feature: AiFeature::SubjectMask,
            mask_models: &[AiMaskModel::Subject],
            gpu_variant: None,
            disk_required: false,
        },
        // --- foreground masking (BiRefNet variants) ---
        RegistryEntry {
            id: "birefnet_fp16",
            name: "Foreground Model (fp16)",
            asset: Some(&BIREFNET_FP16_ASSET),
            filename: BIREFNET_FP16_FILENAME,
            feature: AiFeature::ForegroundMask,
            mask_models: &[AiMaskModel::Foreground],
            gpu_variant: Some(GpuVariant::Fp16),
            disk_required: false,
        },
        RegistryEntry {
            id: "birefnet_fp32",
            name: "Foreground Model (fp32)",
            asset: Some(&BIREFNET_FP32_ASSET),
            filename: BIREFNET_FP32_FILENAME,
            feature: AiFeature::ForegroundMask,
            mask_models: &[AiMaskModel::Foreground],
            gpu_variant: Some(GpuVariant::Fp32),
            disk_required: false,
        },
        // --- shared mask family components (ESAM3/MediaPipe) ---
        RegistryEntry {
            id: "esam3_image_encoder",
            name: "EfficientSAM3 Encoder",
            asset: Some(&ESAM3_IMAGE_ENCODER_ASSET),
            filename: ESAM3_IMAGE_ENCODER_FILENAME,
            feature: AiFeature::SharedMask,
            mask_models: &[AiMaskModel::Sky, AiMaskModel::Text, AiMaskModel::Face],
            gpu_variant: None,
            disk_required: false,
        },
        RegistryEntry {
            id: "esam3_decoder",
            name: "EfficientSAM3 Decoder",
            asset: Some(&ESAM3_DECODER_ASSET),
            filename: ESAM3_DECODER_FILENAME,
            feature: AiFeature::SharedMask,
            mask_models: &[AiMaskModel::Sky, AiMaskModel::Text, AiMaskModel::Face],
            gpu_variant: None,
            disk_required: false,
        },
        RegistryEntry {
            id: "esam3_text_encoder",
            name: "EfficientSAM3 Text Encoder",
            asset: Some(&ESAM3_TEXT_ENCODER_ASSET),
            filename: ESAM3_TEXT_ENCODER_FILENAME,
            feature: AiFeature::SharedMask,
            mask_models: &[AiMaskModel::Text],
            gpu_variant: None,
            disk_required: false,
        },
        RegistryEntry {
            id: "esam3_sky_prompt",
            name: "Sky Prompt",
            asset: Some(&ESAM3_SKY_PROMPT_ASSET),
            filename: ESAM3_SKY_PROMPT_FILENAME,
            feature: AiFeature::SharedMask,
            mask_models: &[AiMaskModel::Sky],
            gpu_variant: None,
            disk_required: false,
        },
        RegistryEntry {
            id: "esam3_face_prompt",
            name: "Face Prompt",
            asset: Some(&ESAM3_FACE_PROMPT_ASSET),
            filename: ESAM3_FACE_PROMPT_FILENAME,
            feature: AiFeature::SharedMask,
            mask_models: &[AiMaskModel::Face],
            gpu_variant: None,
            disk_required: false,
        },
        RegistryEntry {
            id: "face_landmarks",
            name: "Face Landmarks Model",
            asset: Some(&FACE_LANDMARKS_ASSET),
            filename: FACE_LANDMARKS_FILENAME,
            feature: AiFeature::SharedMask,
            mask_models: &[AiMaskModel::Face],
            gpu_variant: None,
            disk_required: false,
        },
        RegistryEntry {
            id: "face_segmenter",
            name: "Face Segmentation Model",
            asset: Some(&FACE_SEGMENTER_ASSET),
            filename: FACE_SEGMENTER_FILENAME,
            feature: AiFeature::SharedMask,
            mask_models: &[AiMaskModel::Face],
            gpu_variant: None,
            disk_required: false,
        },
        RegistryEntry {
            id: "clip_tokenizer",
            name: "CLIP Tokenizer",
            asset: Some(&CLIP_TOKENIZER_ASSET),
            filename: CLIP_TOKENIZER_FILENAME,
            feature: AiFeature::SharedMask,
            mask_models: &[AiMaskModel::Text],
            gpu_variant: None,
            disk_required: false,
        },
        // --- depth masking ---
        RegistryEntry {
            id: "depth_anything",
            name: "Depth Model",
            asset: Some(&DEPTH_ASSET),
            filename: DEPTH_FILENAME,
            feature: AiFeature::DepthMask,
            mask_models: &[AiMaskModel::Depth],
            gpu_variant: None,
            disk_required: false,
        },
        // --- AI denoising ---
        RegistryEntry {
            id: "nind_denoise",
            name: "NIND Denoise Model",
            asset: Some(&DENOISE_ASSET),
            filename: DENOISE_FILENAME,
            feature: AiFeature::Denoise,
            mask_models: &[],
            gpu_variant: None,
            disk_required: false,
        },
        #[cfg(target_env = "ohos")]
        RegistryEntry {
            id: "nind_denoise_ms",
            name: "NIND Denoise (.ms)",
            asset: Some(&DENOISE_MS_ASSET),
            filename: DENOISE_MS_FILENAME,
            feature: AiFeature::Denoise,
            mask_models: &[],
            gpu_variant: None,
            disk_required: true,
        },
        #[cfg(target_env = "ohos")]
        RegistryEntry {
            id: "nind_denoise_om",
            name: "NIND Denoise (.om)",
            asset: None,
            filename: DENOISE_OM_FILENAME,
            feature: AiFeature::Denoise,
            mask_models: &[],
            gpu_variant: None,
            disk_required: true,
        },
        // --- super resolution ---
        RegistryEntry {
            id: "upscale_2xbhi",
            name: "Super Resolution Model",
            asset: Some(&UPSCALE_ASSET),
            filename: UPSCALE_FILENAME,
            feature: AiFeature::Upscale,
            mask_models: &[],
            gpu_variant: None,
            disk_required: false,
        },
        #[cfg(target_env = "ohos")]
        RegistryEntry {
            id: "upscale_om",
            name: "Super Resolution (.om)",
            asset: None,
            filename: UPSCALE_OM_FILENAME,
            feature: AiFeature::Upscale,
            mask_models: &[],
            gpu_variant: None,
            disk_required: true,
        },
        // --- inpainting ---
        RegistryEntry {
            id: "lama_fp16",
            name: "Inpainting Model",
            asset: Some(&LAMA_ASSET),
            filename: LAMA_FILENAME,
            feature: AiFeature::Inpaint,
            mask_models: &[],
            gpu_variant: None,
            disk_required: false,
        },
        #[cfg(target_env = "ohos")]
        RegistryEntry {
            id: "lama_om",
            name: "LaMa Inpainting (.om)",
            asset: None,
            filename: LAMA_OM_FILENAME,
            feature: AiFeature::Inpaint,
            mask_models: &[],
            gpu_variant: None,
            disk_required: true,
        },
        // --- AI tagging ---
        RegistryEntry {
            id: "clip_model",
            name: "CLIP Model",
            asset: Some(&CLIP_MODEL_ASSET),
            filename: CLIP_MODEL_FILENAME,
            feature: AiFeature::Tagging,
            mask_models: &[],
            gpu_variant: None,
            disk_required: false,
        },
        // --- normal maps ---
        RegistryEntry {
            id: "moge_normal",
            name: "Normal Model",
            asset: Some(&NORMAL_ASSET),
            filename: NORMAL_FILENAME,
            feature: AiFeature::Normal,
            mask_models: &[],
            gpu_variant: None,
            disk_required: false,
        },
    ]
}

/// Assets a mask feature lazily downloads before its first use. This is the
/// single feature → asset mapping (replaces the hand-written match that used
/// to live next to `AiMaskModel`); the BiRefNet fp16/fp32 variant selection
/// rides the `gpu_variant` flags.
pub(crate) fn mask_model_assets(
    model: AiMaskModel,
    gpu_inference: bool,
) -> Vec<&'static ModelAsset> {
    registry()
        .iter()
        .filter(|entry| entry.mask_models.contains(&model))
        .filter(|entry| match entry.gpu_variant {
            Some(GpuVariant::Fp16) => gpu_inference,
            Some(GpuVariant::Fp32) => !gpu_inference,
            None => true,
        })
        .filter_map(|entry| entry.asset)
        .collect()
}

// ---------------------------------------------------------------------------
// Status computation
// ---------------------------------------------------------------------------

/// Cached sha256 verdict keyed by `(len, mtime)`: opening the Settings page
/// must not re-hash gigabytes of model files every time. A `Corrupt`
/// conclusion stays cached until the file actually changes.
struct HashCacheEntry {
    len: u64,
    mtime: Option<SystemTime>,
    ok: bool,
}

static HASH_CACHE: LazyLock<Mutex<HashMap<PathBuf, HashCacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn hash_check(path: &Path, meta: &fs::Metadata, expected: &str) -> AiModelStatus {
    let len = meta.len();
    let mtime = meta.modified().ok();
    let cache = HASH_CACHE.lock().unwrap();
    if let Some(cached) = cache.get(path)
        && cached.len == len
        && cached.mtime == mtime
    {
        return if cached.ok {
            AiModelStatus::Ready
        } else {
            AiModelStatus::Corrupt
        };
    }
    // Drop the lock before the (slow) hash so parallel status scans of other
    // files do not serialize behind this one.
    drop(cache);
    let ok = crate::ai::models::verify_sha256(path, expected).unwrap_or(false);
    HASH_CACHE
        .lock()
        .unwrap()
        .insert(path.to_path_buf(), HashCacheEntry { len, mtime, ok });
    if ok {
        AiModelStatus::Ready
    } else {
        AiModelStatus::Corrupt
    }
}

#[cfg(target_env = "ohos")]
fn bundled_model_size(filename: &str) -> Option<u64> {
    crate::ohos_integration::rawfile_models::size(filename)
}

#[cfg(not(target_env = "ohos"))]
fn bundled_model_size(_filename: &str) -> Option<u64> {
    None
}

fn build_info(entry: &RegistryEntry, models_dir: &Path) -> AiModelInfo {
    let path = models_dir.join(entry.filename);
    let meta = fs::metadata(&path).ok().filter(|meta| meta.is_file());
    let bundled = crate::ai::models::has_bundled_model(entry.filename);
    let (status, size_bytes) = match meta {
        Some(meta) => {
            let size = meta.len();
            let status = match entry.asset.and_then(|asset| asset.sha256) {
                None => AiModelStatus::Ready,
                Some(expected) => hash_check(&path, &meta, expected),
            };
            (status, size)
        }
        // A release HAP can carry the model in its rawfile bundle; report it
        // as available (with the bundled size) before the first extraction.
        None if bundled => (
            AiModelStatus::Bundled,
            bundled_model_size(entry.filename).unwrap_or(0),
        ),
        None => (AiModelStatus::NotInstalled, 0),
    };
    AiModelInfo {
        id: entry.id,
        name: entry.name,
        filename: entry.filename,
        feature: entry.feature,
        status,
        size_bytes,
        bundled,
        disk_required: entry.disk_required,
        downloadable: entry.asset.is_some(),
        sha256: entry.asset.and_then(|asset| asset.sha256),
    }
}

/// Settings → AI model management: list every registry entry with its live
/// status. Disk IO (and first-run sha256 verification of large models) runs
/// on the blocking pool so the IPC call cannot stall the runtime.
#[tauri::command]
pub async fn list_ai_models(app_handle: AppHandle) -> Result<Vec<AiModelInfo>, String> {
    let models_dir = crate::ai::models::get_models_dir(&app_handle).map_err(|e| e.to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        registry()
            .iter()
            .map(|entry| build_info(entry, &models_dir))
            .collect::<Vec<_>>()
    })
    .await
    .map_err(|e| e.to_string())
}
