use crate::ai::processing::{CachedDepthMap, Esam3Features, ImageEmbeddings};
use crate::masks::face::FaceAnalysis;
use anyhow::Result;
use ndarray::{Array, IxDyn};
use ort::editor::{Graph, Model, Node, ONNX_DOMAIN, Opset, Outlet};
use ort::execution_providers::{CPUExecutionProvider, ExecutionProviderDispatch};
use ort::memory::Allocator;
use ort::session::{Output, Session};
use ort::tensor::{Shape, SymbolicDimensions, TensorElementType};
use ort::value::{Tensor, ValueType};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Emitter;
use tokenizers::Tokenizer;
use tokio::sync::Mutex as TokioMutex;

const ENCODER_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/sam_vit_b_01ec64_encoder.onnx?download=true";

const DECODER_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/sam_vit_b_01ec64_decoder.onnx?download=true";

const ENCODER_FILENAME: &str = "sam_vit_b_01ec64_encoder.onnx";

const DECODER_FILENAME: &str = "sam_vit_b_01ec64_decoder.onnx";

const ENCODER_SHA256: &str = "16ab73d9c824886f0de2938c19df22fb9ec3deebfd0de58e65177e479213d7d1";

const DECODER_SHA256: &str = "85d0d672cf5b7fe763edcde429e5533e62f674af4b15c7d688b7673b0ef00bf7";

const BIREFNET_FP16_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/birefnet_general_lite_fp16.onnx?download=true";

const BIREFNET_FP16_FILENAME: &str = "birefnet_general_lite_fp16.onnx";

const BIREFNET_FP16_SHA256: &str =
    "3b72cb3c2138605ced0a4b271e8d13d3b3ca2483cf1665b098b6851767420fa1";

const BIREFNET_FP32_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/birefnet_general_lite.onnx?download=true";

const BIREFNET_FP32_FILENAME: &str = "birefnet_general_lite.onnx";

const BIREFNET_FP32_SHA256: &str =
    "5600024376f572a557870a5eb0afb1e5961636bef4e1e22132025467d0f03333";

const ESAM3_IMAGE_ENCODER_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/efficientsam3_ev_m_image_encoder.onnx?download=true";

const ESAM3_IMAGE_ENCODER_FILENAME: &str = "efficientsam3_ev_m_image_encoder.onnx";

const ESAM3_IMAGE_ENCODER_SHA256: &str =
    "2ed08306579138120dbf9f1110fd31784d1c20fe1b67fa2187a0a2a6d7c82729";

const ESAM3_DECODER_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/efficientsam3_ev_m_decoder.onnx?download=true";

const ESAM3_DECODER_FILENAME: &str = "efficientsam3_ev_m_decoder.onnx";

const ESAM3_DECODER_SHA256: &str =
    "16accc22a07ee99c73ef8a14e286ec567f94c116e0d8efa1a3a6c0bc9a2596a8";

const ESAM3_TEXT_ENCODER_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/efficientsam3_ev_m_text_encoder.onnx?download=true";

const ESAM3_TEXT_ENCODER_FILENAME: &str = "efficientsam3_ev_m_text_encoder.onnx";

const ESAM3_TEXT_ENCODER_SHA256: &str =
    "3065ea6116f21f6e22bc70a0c041e5da9ae05fdec7b48df953394f5657609590";

const ESAM3_SKY_PROMPT_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/efficientsam3_sky_prompt.bin?download=true";

const ESAM3_SKY_PROMPT_FILENAME: &str = "efficientsam3_sky_prompt.bin";

const ESAM3_SKY_PROMPT_SHA256: &str =
    "b8dc43661db640f2e3881466252d154906d90896eb3ed9dbc747bec87a97d180";

const ESAM3_FACE_PROMPT_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/efficientsam3_face_prompt.bin?download=true";

const ESAM3_FACE_PROMPT_FILENAME: &str = "efficientsam3_face_prompt.bin";

const ESAM3_FACE_PROMPT_SHA256: &str =
    "8fdbe63ef853756b1da9cdc642c277a6e422452f3feaf35f4297e6aea3a1ffb9";

const FACE_LANDMARKS_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/mediapipe_face_landmarks.onnx?download=true";

const FACE_LANDMARKS_FILENAME: &str = "mediapipe_face_landmarks.onnx";

const FACE_LANDMARKS_SHA256: &str =
    "6524d596b4d9151b0d8d590844566261f21de5186ac38125bdcd1de6974b0bb4";

const FACE_SEGMENTER_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/mediapipe_selfie_multiclass.onnx?download=true";

const FACE_SEGMENTER_FILENAME: &str = "mediapipe_selfie_multiclass.onnx";

const FACE_SEGMENTER_SHA256: &str =
    "a198470a4edec350a33b8182c149e417effc5e61c0506b947f226e7a81b9a08e";

const CLIP_MODEL_URL: &str =
    "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/clip_model.onnx?download=true";

const CLIP_MODEL_FILENAME: &str = "clip_model.onnx";

const CLIP_TOKENIZER_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/clip_tokenizer.json?download=true";

const CLIP_TOKENIZER_FILENAME: &str = "clip_tokenizer.json";

const CLIP_MODEL_SHA256: &str = "57879bb1c23cdeb350d23569dd251ed4b740a96d747c529e94a2bb8040ac5d00";

const DENOISE_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/nind_denoise_utnet_684.onnx?download=true";

const DENOISE_FILENAME: &str = "nind_denoise_utnet_684.onnx";

const DENOISE_SHA256: &str = "ee3586279d514df557ff3f7dec6df37fafc51ba5d3a3435b2cc9ac2d9017e7fe";

// NPU denoise models (OpenHarmony only). The .ms build comes from
// scripts/convert-nind-ms.ps1 (ONNX -> Pad surgery -> converter_lite 2.7.0);
// full recipe + pinned hashes: docs/AI_DENOISE_HW_ACCELERATION.md section 5.6.
#[cfg(target_env = "ohos")]
const DENOISE_MS_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/nind_denoise_utnet_684.ms?download=true";
#[cfg(target_env = "ohos")]
const DENOISE_MS_FILENAME: &str = "nind_denoise_utnet_684.ms";
#[cfg(target_env = "ohos")]
const DENOISE_MS_SHA256: &str = "05d7580e5a6e3c18bd286705f7524090cd69abe4b78067c2a173b322ef151b3b";

// CANN (.om) denoise build, sideloaded; same nind lineage after Pad→Slice
// surgery -> omg (504 is the fixed tile size all TileParams variants use).
// NCHW I/O (no NHWC conversion).
#[cfg(target_env = "ohos")]
pub const DENOISE_OM_FILENAME: &str = "nind_v2.om";
/// HIAI_HCL perf-mode optimum for small-op-dense CNN graphs (§8.4: NIND
/// 1.8× over the default 2; transformer/FFC graphs keep 2).
#[cfg(target_env = "ohos")]
const OM_PERF_MODE_CNN: i32 = 4;

const UPSCALE_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/2xBHI_small_span_rapidraw.onnx?download=true";

const UPSCALE_FILENAME: &str = "2xBHI_small_span_rapidraw.onnx";

const UPSCALE_SHA256: &str = "9d3a51deab2518cde257048180070b078840f2aa32326b8bf0e86f15f2a689a0";

const UPSCALE_SPATIAL_DIMS: [&str; 2] = ["h", "w"];

/// Side-loaded CANN `.om` for the RapidRes upscale model (same provisioning
/// channel as `nind_v2.om`: never bundled, always SoC-specific, built with
/// `omg --platform=<soc>` per docs/RAPIDRES_NPU_FEASIBILITY.md).
#[cfg(target_env = "ohos")]
pub const UPSCALE_OM_FILENAME: &str = "2xBHI_small_span_rapidraw.om";

const LAMA_URL: &str =
    "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/lama_fp16.onnx?download=true";

const LAMA_FILENAME: &str = "lama_fp16.onnx";

const LAMA_SHA256: &str = "2d6be6277c400d6f1b91819737f7c3da935e5c63d1b521d393be1196a2bfa82c";

const DEPTH_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/depth_anything_v2_vits.onnx?download=true";

const DEPTH_FILENAME: &str = "depth_anything_v2_vits.onnx";

const DEPTH_SHA256: &str = "d2b11a11c1d4a12b47608fa65a17ee9a4c605b55ee1730c8e3b526304f2562be";

const NORMAL_URL: &str = "https://hf-mirror.com/CyberTimon/RapidRAW-Models/resolve/main/moge-2-vits-normal-onnx.onnx?download=true";

const NORMAL_FILENAME: &str = "moge-2-vits-normal-onnx.onnx";

const NORMAL_SHA256: &str = "24eacb5dc7a2c54c7bc98f7de085ffbed79ad006ea5b664c2c2cdc02ff3a52f0";

type SharedSession = Arc<Mutex<Session>>;

const GPU_IDLE: u8 = 0;

const GPU_ACTIVE: u8 = 1;

const GPU_FAILED: u8 = 2;

const GPU_DISABLED: u8 = 3;

static GPU_STATUS: AtomicU8 = AtomicU8::new(GPU_IDLE);

struct ModelAsset {
    filename: &'static str,
    url: &'static str,
    sha256: Option<&'static str>,
    name: &'static str,
}

const SAM_ENCODER_ASSET: ModelAsset = ModelAsset {
    filename: ENCODER_FILENAME,
    url: ENCODER_URL,
    sha256: Some(ENCODER_SHA256),
    name: "SAM Encoder",
};

const SAM_DECODER_ASSET: ModelAsset = ModelAsset {
    filename: DECODER_FILENAME,
    url: DECODER_URL,
    sha256: Some(DECODER_SHA256),
    name: "SAM Decoder",
};

const BIREFNET_FP16_ASSET: ModelAsset = ModelAsset {
    filename: BIREFNET_FP16_FILENAME,
    url: BIREFNET_FP16_URL,
    sha256: Some(BIREFNET_FP16_SHA256),
    name: "Foreground Model",
};

const BIREFNET_FP32_ASSET: ModelAsset = ModelAsset {
    filename: BIREFNET_FP32_FILENAME,
    url: BIREFNET_FP32_URL,
    sha256: Some(BIREFNET_FP32_SHA256),
    name: "Foreground Model",
};

const ESAM3_IMAGE_ENCODER_ASSET: ModelAsset = ModelAsset {
    filename: ESAM3_IMAGE_ENCODER_FILENAME,
    url: ESAM3_IMAGE_ENCODER_URL,
    sha256: Some(ESAM3_IMAGE_ENCODER_SHA256),
    name: "EfficientSAM3 Encoder",
};

const ESAM3_DECODER_ASSET: ModelAsset = ModelAsset {
    filename: ESAM3_DECODER_FILENAME,
    url: ESAM3_DECODER_URL,
    sha256: Some(ESAM3_DECODER_SHA256),
    name: "EfficientSAM3 Decoder",
};

const ESAM3_TEXT_ENCODER_ASSET: ModelAsset = ModelAsset {
    filename: ESAM3_TEXT_ENCODER_FILENAME,
    url: ESAM3_TEXT_ENCODER_URL,
    sha256: Some(ESAM3_TEXT_ENCODER_SHA256),
    name: "EfficientSAM3 Text Encoder",
};

const ESAM3_SKY_PROMPT_ASSET: ModelAsset = ModelAsset {
    filename: ESAM3_SKY_PROMPT_FILENAME,
    url: ESAM3_SKY_PROMPT_URL,
    sha256: Some(ESAM3_SKY_PROMPT_SHA256),
    name: "Sky Prompt",
};

const ESAM3_FACE_PROMPT_ASSET: ModelAsset = ModelAsset {
    filename: ESAM3_FACE_PROMPT_FILENAME,
    url: ESAM3_FACE_PROMPT_URL,
    sha256: Some(ESAM3_FACE_PROMPT_SHA256),
    name: "Face Prompt",
};

const FACE_LANDMARKS_ASSET: ModelAsset = ModelAsset {
    filename: FACE_LANDMARKS_FILENAME,
    url: FACE_LANDMARKS_URL,
    sha256: Some(FACE_LANDMARKS_SHA256),
    name: "Face Landmarks Model",
};

const FACE_SEGMENTER_ASSET: ModelAsset = ModelAsset {
    filename: FACE_SEGMENTER_FILENAME,
    url: FACE_SEGMENTER_URL,
    sha256: Some(FACE_SEGMENTER_SHA256),
    name: "Face Segmentation Model",
};

const DEPTH_ASSET: ModelAsset = ModelAsset {
    filename: DEPTH_FILENAME,
    url: DEPTH_URL,
    sha256: Some(DEPTH_SHA256),
    name: "Depth Model",
};

const CLIP_MODEL_ASSET: ModelAsset = ModelAsset {
    filename: CLIP_MODEL_FILENAME,
    url: CLIP_MODEL_URL,
    sha256: Some(CLIP_MODEL_SHA256),
    name: "CLIP Model",
};

const CLIP_TOKENIZER_ASSET: ModelAsset = ModelAsset {
    filename: CLIP_TOKENIZER_FILENAME,
    url: CLIP_TOKENIZER_URL,
    sha256: None,
    name: "CLIP Tokenizer",
};

const DENOISE_ASSET: ModelAsset = ModelAsset {
    filename: DENOISE_FILENAME,
    url: DENOISE_URL,
    sha256: Some(DENOISE_SHA256),
    name: "NIND Denoise Model",
};

const UPSCALE_ASSET: ModelAsset = ModelAsset {
    filename: UPSCALE_FILENAME,
    url: UPSCALE_URL,
    sha256: Some(UPSCALE_SHA256),
    name: "Super Resolution Model",
};

const LAMA_ASSET: ModelAsset = ModelAsset {
    filename: LAMA_FILENAME,
    url: LAMA_URL,
    sha256: Some(LAMA_SHA256),
    name: "Inpainting Model",
};

const NORMAL_ASSET: ModelAsset = ModelAsset {
    filename: NORMAL_FILENAME,
    url: NORMAL_URL,
    sha256: Some(NORMAL_SHA256),
    name: "Normal Model",
};

// ---------------------------------------------------------------------------
// Dual-path model loading (OpenHarmony packaging)
//
// Release HAPs carry a curated high-frequency model set inside the package
// (resources/rawfile/models/, injected by scripts/build-ohos.ps1 -Release);
// debug builds ship none. Bundled models load straight from the HAP via
// commit_from_memory - nothing is extracted or copied to app data. Anything
// not bundled follows the classic path: download into the app-data models
// dir (sha256-verified, then commit_from_file). A file already present in
// the models dir always wins over the bundle, keeping the download dir an
// override / hot-update channel.
// ---------------------------------------------------------------------------

/// Where a model's bytes come from when a session is built.
enum ModelSource {
    /// Bytes read from the HAP rawfile bundle (OHOS release builds only).
    Bundled { filename: String, bytes: Vec<u8> },
    /// File under the app-data models dir (download/sideload channel).
    File(PathBuf),
}

impl ModelSource {
    fn filename(&self) -> &str {
        match self {
            ModelSource::Bundled { filename, .. } => filename,
            ModelSource::File(path) => path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("?"),
        }
    }

    fn resolve(filename: &str, models_dir: &Path) -> ModelSource {
        let path = models_dir.join(filename);
        if path.exists() || !has_bundled_model(filename) {
            ModelSource::File(path)
        } else {
            match read_bundled_model(filename) {
                Some(bytes) => ModelSource::Bundled {
                    filename: filename.to_string(),
                    bytes,
                },
                None => ModelSource::File(path),
            }
        }
    }
}

#[cfg(target_env = "ohos")]
fn has_bundled_model(filename: &str) -> bool {
    crate::ohos_integration::rawfile_models::has(filename)
}

#[cfg(not(target_env = "ohos"))]
fn has_bundled_model(_filename: &str) -> bool {
    false
}

#[cfg(target_env = "ohos")]
fn read_bundled_model(filename: &str) -> Option<Vec<u8>> {
    crate::ohos_integration::rawfile_models::read(filename)
}

#[cfg(not(target_env = "ohos"))]
fn read_bundled_model(_filename: &str) -> Option<Vec<u8>> {
    None
}

/// Materialise a model bundled in the HAP rawfile set into the app-data models
/// dir. The CANN (`.om`) and MindSpore Lite (`.ms`) engines take a filesystem
/// path, not a memory buffer, so the bundle is extracted once and then reused
/// exactly like a downloaded/sideloaded model. Returns `None` when the model is
/// neither on disk nor in the bundle. No-op when the file already exists.
#[cfg(target_env = "ohos")]
fn provision_bundled_model(
    app_handle: &tauri::AppHandle,
    filename: &str,
) -> Result<Option<PathBuf>> {
    let dest = get_models_dir(app_handle)?.join(filename);
    if dest.exists() {
        return Ok(Some(dest));
    }
    if !has_bundled_model(filename) {
        return Ok(None);
    }
    let bytes = read_bundled_model(filename)
        .ok_or_else(|| anyhow::anyhow!("bundled model '{filename}' could not be read"))?;
    persist_downloaded_asset(&dest, &bytes)?;
    log::info!(
        "[ai] extracted bundled model '{filename}' ({} bytes) to {}",
        bytes.len(),
        dest.display()
    );
    Ok(Some(dest))
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum AiMaskModel {
    Subject,
    Foreground,
    Sky,
    Text,
    Face,
    Depth,
}

impl AiMaskModel {
    fn assets(self, gpu_inference: bool) -> &'static [ModelAsset] {
        match self {
            AiMaskModel::Subject => &[SAM_ENCODER_ASSET, SAM_DECODER_ASSET],
            AiMaskModel::Foreground if gpu_inference => &[BIREFNET_FP16_ASSET],
            AiMaskModel::Foreground => &[BIREFNET_FP32_ASSET],
            AiMaskModel::Sky => &[
                ESAM3_IMAGE_ENCODER_ASSET,
                ESAM3_DECODER_ASSET,
                ESAM3_SKY_PROMPT_ASSET,
            ],
            AiMaskModel::Text => &[
                ESAM3_IMAGE_ENCODER_ASSET,
                ESAM3_DECODER_ASSET,
                ESAM3_TEXT_ENCODER_ASSET,
                CLIP_TOKENIZER_ASSET,
            ],
            AiMaskModel::Face => &[
                ESAM3_IMAGE_ENCODER_ASSET,
                ESAM3_DECODER_ASSET,
                ESAM3_FACE_PROMPT_ASSET,
                FACE_LANDMARKS_ASSET,
                FACE_SEGMENTER_ASSET,
            ],
            AiMaskModel::Depth => &[DEPTH_ASSET],
        }
    }
}

pub struct AiModels {
    models_dir: PathBuf,
    use_gpu: bool,
    downloaded: Mutex<HashSet<AiMaskModel>>,
    sam_encoder: Mutex<Option<SharedSession>>,
    sam_decoder: Mutex<Option<SharedSession>>,
    birefnet: Mutex<Option<SharedSession>>,
    esam3_image_encoder: Mutex<Option<SharedSession>>,
    esam3_decoder: Mutex<Option<SharedSession>>,
    esam3_text_encoder: Mutex<Option<SharedSession>>,
    esam3_sky_prompt: Mutex<Option<Arc<Esam3Prompt>>>,
    esam3_face_prompt: Mutex<Option<Arc<Esam3Prompt>>>,
    clip_tokenizer: Mutex<Option<Arc<Tokenizer>>>,
    face_landmarks: Mutex<Option<SharedSession>>,
    face_segmenter: Mutex<Option<SharedSession>>,
    depth_anything: Mutex<Option<SharedSession>>,
}

impl AiModels {
    fn session(
        &self,
        slot: &Mutex<Option<SharedSession>>,
        filename: &str,
        use_gpu: bool,
    ) -> Result<SharedSession> {
        let mut slot = slot.lock().unwrap();
        if let Some(session) = slot.as_ref() {
            return Ok(Arc::clone(session));
        }
        let session = Arc::new(Mutex::new(build_session(
            &ModelSource::resolve(filename, &self.models_dir),
            SessionKind::OneShot,
            self.use_gpu && use_gpu,
            None,
        )?));
        *slot = Some(Arc::clone(&session));
        Ok(session)
    }

    pub fn sam_encoder(&self) -> Result<SharedSession> {
        self.session(&self.sam_encoder, ENCODER_FILENAME, false)
    }

    pub fn sam_decoder(&self) -> Result<SharedSession> {
        self.session(&self.sam_decoder, DECODER_FILENAME, false)
    }

    fn gpu_inference(&self) -> bool {
        self.use_gpu && gpu_allowed_on_platform() && gpu_execution_provider().is_some()
    }

    pub fn birefnet(&self) -> Result<SharedSession> {
        let filename = if self.gpu_inference() {
            BIREFNET_FP16_FILENAME
        } else {
            BIREFNET_FP32_FILENAME
        };
        self.session(&self.birefnet, filename, gpu_allowed_on_platform())
    }

    pub fn esam3_image_encoder(&self) -> Result<SharedSession> {
        self.session(
            &self.esam3_image_encoder,
            ESAM3_IMAGE_ENCODER_FILENAME,
            gpu_allowed_on_platform(),
        )
    }

    pub fn esam3_decoder(&self) -> Result<SharedSession> {
        self.session(
            &self.esam3_decoder,
            ESAM3_DECODER_FILENAME,
            gpu_allowed_on_platform(),
        )
    }

    pub fn depth_anything(&self) -> Result<SharedSession> {
        self.session(&self.depth_anything, DEPTH_FILENAME, true)
    }

    pub fn face_landmarks(&self) -> Result<SharedSession> {
        self.session(&self.face_landmarks, FACE_LANDMARKS_FILENAME, false)
    }

    pub fn face_segmenter(&self) -> Result<SharedSession> {
        self.session(&self.face_segmenter, FACE_SEGMENTER_FILENAME, false)
    }

    pub fn esam3_sky_prompt(&self) -> Result<Arc<Esam3Prompt>> {
        self.stored_prompt(&self.esam3_sky_prompt, ESAM3_SKY_PROMPT_FILENAME)
    }

    pub fn esam3_face_prompt(&self) -> Result<Arc<Esam3Prompt>> {
        self.stored_prompt(&self.esam3_face_prompt, ESAM3_FACE_PROMPT_FILENAME)
    }

    fn stored_prompt(
        &self,
        slot: &Mutex<Option<Arc<Esam3Prompt>>>,
        filename: &str,
    ) -> Result<Arc<Esam3Prompt>> {
        let mut slot = slot.lock().unwrap();
        if let Some(prompt) = slot.as_ref() {
            return Ok(Arc::clone(prompt));
        }
        let bytes = fs::read(self.models_dir.join(filename))
            .ok()
            .or_else(|| read_bundled_model(filename))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "prompt file {} not found in models dir or HAP bundle",
                    filename
                )
            })?;
        let values: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&chunk| f32::from_le_bytes(chunk))
            .collect();
        let feature_len = ESAM3_PROMPT_TOKENS * ESAM3_EMBED_DIM;
        if values.len() != feature_len + ESAM3_PROMPT_TOKENS {
            return Err(anyhow::anyhow!("Invalid prompt embedding {}", filename));
        }
        let prompt = Arc::new(Esam3Prompt {
            features: Array::from_shape_vec(
                IxDyn(&[ESAM3_PROMPT_TOKENS, 1, ESAM3_EMBED_DIM]),
                values[..feature_len].to_vec(),
            )?,
            padding: Array::from_shape_vec(
                IxDyn(&[1, ESAM3_PROMPT_TOKENS]),
                values[feature_len..].to_vec(),
            )?,
        });
        *slot = Some(Arc::clone(&prompt));
        Ok(prompt)
    }

    pub fn esam3_text_prompt(&self, text: &str) -> Result<Esam3Prompt> {
        let tokenizer = {
            let mut slot = self.clip_tokenizer.lock().unwrap();
            match slot.as_ref() {
                Some(tokenizer) => Arc::clone(tokenizer),
                None => {
                    let tokenizer = Arc::new(
                        Tokenizer::from_file(self.models_dir.join(CLIP_TOKENIZER_FILENAME))
                            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
                    );
                    *slot = Some(Arc::clone(&tokenizer));
                    tokenizer
                }
            }
        };
        let encoding = tokenizer
            .encode(text.trim(), true)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let ids = encoding.get_ids();
        let mut tokens = Array::<i64, _>::zeros((1, ESAM3_PROMPT_TOKENS));
        let kept = ids.len().min(ESAM3_PROMPT_TOKENS);
        for (i, &id) in ids[..kept].iter().enumerate() {
            tokens[[0, i]] = id as i64;
        }
        if let Some(&end) = ids.last()
            && ids.len() > ESAM3_PROMPT_TOKENS
        {
            tokens[[0, ESAM3_PROMPT_TOKENS - 1]] = end as i64;
        }

        let session = self.session(&self.esam3_text_encoder, ESAM3_TEXT_ENCODER_FILENAME, false)?;
        let mut session = session.lock().unwrap();
        let outputs = session.run(ort::inputs!["tokens" => Tensor::from_array(tokens)?])?;
        Ok(Esam3Prompt {
            features: outputs["language_features"]
                .try_extract_array::<f32>()?
                .to_owned(),
            padding: outputs["language_padding"]
                .try_extract_array::<f32>()?
                .to_owned(),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SessionKind {
    OneShot,
    Persistent,
}

#[cfg(target_os = "windows")]
fn high_performance_dml_device_id() -> Option<i32> {
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory2, DXGI_CREATE_FACTORY_FLAGS, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE,
        IDXGIAdapter1, IDXGIFactory6,
    };

    unsafe {
        let factory: IDXGIFactory6 = CreateDXGIFactory2(DXGI_CREATE_FACTORY_FLAGS(0)).ok()?;
        let preferred: IDXGIAdapter1 = factory
            .EnumAdapterByGpuPreference(0, DXGI_GPU_PREFERENCE_HIGH_PERFORMANCE)
            .ok()?;
        let luid = preferred.GetDesc1().ok()?.AdapterLuid;
        (0..)
            .map_while(|i| factory.EnumAdapters1(i).ok())
            .position(|adapter| {
                adapter
                    .GetDesc1()
                    .is_ok_and(|desc| desc.AdapterLuid == luid)
            })
            .map(|index| index as i32)
    }
}

#[cfg(target_os = "windows")]
fn gpu_execution_provider() -> Option<ExecutionProviderDispatch> {
    Some(
        ort::execution_providers::DirectMLExecutionProvider::default()
            .with_device_id(high_performance_dml_device_id().unwrap_or(0))
            .build()
            .error_on_failure(),
    )
}

#[cfg(target_os = "macos")]
fn gpu_execution_provider() -> Option<ExecutionProviderDispatch> {
    use ort::execution_providers::coreml::{CoreMLComputeUnits, CoreMLModelFormat};

    Some(
        ort::execution_providers::CoreMLExecutionProvider::default()
            .with_model_format(CoreMLModelFormat::MLProgram)
            .with_compute_units(CoreMLComputeUnits::All)
            .build()
            .error_on_failure(),
    )
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn gpu_execution_provider() -> Option<ExecutionProviderDispatch> {
    None
}

fn gpu_allowed_on_platform() -> bool {
    !cfg!(target_os = "macos")
}

type GpuModelPatch = fn() -> ort::Result<(Model, Vec<Output>)>;

fn i64_initializer(values: &[i64]) -> ort::Result<Tensor<i64>> {
    let mut tensor = Tensor::<i64>::new(&Allocator::default(), [values.len()])?;
    tensor.extract_tensor_mut().1.copy_from_slice(values);
    Ok(tensor)
}

fn create_session(
    source: &ModelSource,
    kind: SessionKind,
    gpu_provider: Option<ExecutionProviderDispatch>,
    gpu_patch: Option<GpuModelPatch>,
    fixed_dims: &[(&str, i64)],
) -> Result<Session> {
    let use_gpu = gpu_provider.is_some();
    let cpu_provider = CPUExecutionProvider::default()
        .with_arena_allocator(kind == SessionKind::Persistent)
        .build();
    let providers: Vec<_> = gpu_provider.into_iter().chain([cpu_provider]).collect();

    let mut builder = Session::builder()?;
    if use_gpu || kind == SessionKind::OneShot {
        builder = builder.with_memory_pattern(false)?;
    }
    if use_gpu && kind == SessionKind::OneShot {
        builder = builder.with_config_entry("ep.dml.disable_memory_arena", "1")?;
    }
    for &(name, size) in fixed_dims {
        builder = builder.with_dimension_override(name, size)?;
    }
    let builder = builder.with_execution_providers(providers)?;
    match gpu_patch {
        Some(patch) => {
            let (model, outputs) = patch()?;
            let mut session = match source {
                ModelSource::File(path) => builder.edit_from_file(path)?,
                ModelSource::Bundled { bytes, .. } => builder.edit_from_memory(bytes)?,
            };
            session.apply_model(&model)?;
            let mut session = session.into_session()?;
            session.outputs = outputs;
            Ok(session)
        }
        None => match source {
            ModelSource::File(path) => Ok(builder.commit_from_file(path)?),
            ModelSource::Bundled { bytes, .. } => Ok(builder.commit_from_memory(bytes)?),
        },
    }
}

fn build_session(
    source: &ModelSource,
    kind: SessionKind,
    use_gpu: bool,
    gpu_patch: Option<GpuModelPatch>,
) -> Result<Session> {
    build_session_with_dims(source, kind, use_gpu, gpu_patch, &[])
}

fn build_session_with_dims(
    source: &ModelSource,
    kind: SessionKind,
    use_gpu: bool,
    gpu_patch: Option<GpuModelPatch>,
    fixed_dims: &[(&str, i64)],
) -> Result<Session> {
    if use_gpu
        && GPU_STATUS.load(Ordering::Acquire) < GPU_FAILED
        && let Some(gpu_provider) = gpu_execution_provider()
    {
        match create_session(source, kind, Some(gpu_provider), gpu_patch, fixed_dims) {
            Ok(session) => {
                let _ = GPU_STATUS.compare_exchange(
                    GPU_IDLE,
                    GPU_ACTIVE,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
                return Ok(session);
            }
            Err(e) => log::warn!(
                "GPU acceleration unavailable for {:?}, falling back to CPU: {}",
                source.filename(),
                e
            ),
        }
    }
    create_session(source, kind, None, None, fixed_dims)
}

pub fn gpu_acceleration_enabled(app_handle: &tauri::AppHandle) -> bool {
    GPU_STATUS.load(Ordering::Acquire) < GPU_FAILED
        && crate::app::settings::load_settings(app_handle.clone())
            .map(|settings| settings.ai_gpu_acceleration)
            .unwrap_or(true)
}

pub fn gpu_inference<T>(result: ort::Result<T>) -> Result<T> {
    result.map_err(|e| {
        if GPU_STATUS
            .compare_exchange(GPU_ACTIVE, GPU_FAILED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            log::warn!(
                "GPU inference failed, using the CPU for AI models from now on: {}",
                e
            );
        }
        e.into()
    })
}

pub fn reset_ai_sessions(ai_state_mutex: &Mutex<Option<AiState>>) {
    if let Some(ai_state) = ai_state_mutex.lock().unwrap().as_mut() {
        ai_state.release_sessions();
    }
    GPU_STATUS.store(GPU_IDLE, Ordering::Release);
}

pub struct ClipModels {
    pub model: Mutex<Session>,
    pub tokenizer: Tokenizer,
}

fn get_models_dir(app_handle: &tauri::AppHandle) -> Result<PathBuf> {
    let models_dir = crate::app_paths::app_data_dir(app_handle)?.join("models");
    if !models_dir.exists() {
        fs::create_dir_all(&models_dir)?;
    }
    Ok(models_dir)
}

/// Report whether the side-loaded CANN denoise model is present in the models
/// directory, plus its size. The `.om` is a per-SoC artifact that is not
/// shipped with the app, so Settings uses this to tell the user whether the
/// NPU backend can actually engage or will silently fall back to CPU.
#[cfg(target_env = "ohos")]
pub fn denoise_om_status(app_handle: &tauri::AppHandle) -> Result<(bool, u64)> {
    let path = get_models_dir(app_handle)?.join(DENOISE_OM_FILENAME);
    match fs::metadata(&path) {
        Ok(meta) => Ok((true, meta.len())),
        // A release HAP can carry the .om in its rawfile bundle; report it as
        // available before the first extraction (Settings copy stays honest).
        Err(_) => Ok((
            crate::ohos_integration::rawfile_models::has(DENOISE_OM_FILENAME),
            crate::ohos_integration::rawfile_models::size(DENOISE_OM_FILENAME).unwrap_or(0),
        )),
    }
}

/// Initialise ONNX Runtime once, lazily. `ort::init()` panics (ort lib.rs:111)
/// when the dylib is missing, so a plain `?` is not enough — catch the panic
/// and turn it into a normal error the AI commands can surface (BUILD-18).
/// On OpenHarmony the dylib may legitimately be absent from a stripped build;
/// AI features degrade gracefully when this returns Err.
pub fn ensure_ort_available() -> Result<(), String> {
    static INIT: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    INIT.get_or_init(|| {
        let init = std::panic::catch_unwind(|| ort::init().with_name("RapidRAW-AI").commit());
        match init {
            Ok(Ok(_)) => {
                log::info!("[ai] ONNX Runtime initialised");
                Ok(())
            }
            Ok(Err(error)) => {
                let message = format!(
                    "ONNX Runtime is unavailable on this device — AI features are disabled: {error}"
                );
                log::error!("[ai] {message}");
                Err(message)
            }
            Err(_) => {
                let message = "ONNX Runtime is missing from this build — AI features are disabled"
                    .to_string();
                log::error!("[ai] {message}");
                Err(message)
            }
        }
    })
    .clone()
}

fn persist_downloaded_asset(dest: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        return Err(anyhow::anyhow!(
            "Downloaded asset for {} was empty",
            dest.display()
        ));
    }

    let parent = dest.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "Cannot determine parent directory for downloaded asset {}",
            dest.display()
        )
    })?;
    fs::create_dir_all(parent)?;

    let file_name = dest
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid downloaded asset path {}", dest.display()))?;
    let tmp_path = dest.with_file_name(format!(".{}.download", file_name));

    {
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }

    if let Err(rename_error) = fs::rename(&tmp_path, dest) {
        // A stale destination is the common recoverable case on a normal file
        // system: drop it and retry the rename once. Both steps may themselves
        // fail (e.g. a read-only parent on OHOS), in which case the copy
        // fallback below still gets a chance.
        if dest.exists()
            && !dest.is_dir()
            && fs::remove_file(dest).is_ok()
            && fs::rename(&tmp_path, dest).is_ok()
        {
            return Ok(());
        }
        // Some OHOS file systems (the FUSE app-data mount) deny rename() while
        // allowing ordinary writes, so retrying cannot help. Fall back to a
        // plain byte copy and drop the temp file (D-31).
        log::warn!(
            "[ai] rename({}) failed ({}); falling back to byte copy",
            dest.display(),
            rename_error
        );
        fs::copy(&tmp_path, dest)?;
        let _ = fs::remove_file(&tmp_path);
    }
    Ok(())
}

async fn download_model(
    app_handle: &tauri::AppHandle,
    asset: &ModelAsset,
    dest: &Path,
) -> Result<()> {
    let mut response = reqwest::get(asset.url).await?.error_for_status()?;
    let total = response.content_length().unwrap_or(0);
    let mut bytes = Vec::with_capacity(total as usize);
    let mut last_progress = Instant::now();
    while let Some(chunk) = response.chunk().await? {
        bytes.extend_from_slice(&chunk);
        if total > 0 && last_progress.elapsed() >= Duration::from_millis(200) {
            last_progress = Instant::now();
            let _ = app_handle.emit(
                "ai-model-download-progress",
                serde_json::json!({ "name": asset.name, "current": bytes.len(), "total": total }),
            );
        }
    }
    persist_downloaded_asset(dest, &bytes)
}

fn verify_sha256(path: &Path, expected_hash: &str) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    let hash = hasher.finalize();
    let hex_hash = hex::encode(hash);
    Ok(hex_hash == expected_hash)
}

async fn download_and_verify_model(
    app_handle: &tauri::AppHandle,
    models_dir: &Path,
    asset: &ModelAsset,
) -> Result<()> {
    let dest_path = models_dir.join(asset.filename);
    let is_valid = match asset.sha256 {
        Some(expected_hash) => verify_sha256(&dest_path, expected_hash)?,
        None => dest_path.exists(),
    };

    if !is_valid {
        // Release HAPs bundle the high-frequency models in rawfile; a bundled
        // copy only counts when it matches the pinned sha256. This branch is
        // only taken when there is no local file at all: a file that exists
        // but fails verification is replaced by the re-download below, and
        // ModelSource::resolve prefers an existing file over the bundle. On
        // a bundled mismatch - or when nothing is bundled (debug builds) -
        // fall through to the download path.
        if !dest_path.exists()
            && let Some(bytes) = read_bundled_model(asset.filename)
        {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let bundled_hash = hex::encode(hasher.finalize());
            let bundled_valid = match asset.sha256 {
                Some(expected_hash) => bundled_hash == expected_hash,
                None => true,
            };
            if bundled_valid {
                log::info!(
                    "Using bundled model {} (HAP rawfile bundle)",
                    asset.filename
                );
                return Ok(());
            }
            log::warn!(
                "Bundled model {} failed sha256 verification, downloading instead",
                asset.filename
            );
        }
        if dest_path.exists() {
            println!("Model {} has incorrect hash. Re-downloading.", asset.name);
            fs::remove_file(&dest_path)?;
        }
        let _ = app_handle.emit("ai-model-download-start", asset.name);
        let download_result = async {
            download_model(app_handle, asset, &dest_path).await?;
            if let Some(expected_hash) = asset.sha256
                && !verify_sha256(&dest_path, expected_hash)?
            {
                return Err(anyhow::anyhow!(
                    "Failed to verify model {} after download. Hash mismatch.",
                    asset.name
                ));
            }
            Ok(())
        }
        .await;
        let _ = app_handle.emit(
            "ai-model-download-finish",
            serde_json::json!({
                "name": asset.name,
                "error": download_result.as_ref().err().map(|e| e.to_string()),
            }),
        );
        download_result?;
    }
    Ok(())
}

pub async fn get_or_init_ai_models(
    app_handle: &tauri::AppHandle,
    ai_state_mutex: &Mutex<Option<AiState>>,
    ai_init_lock: &TokioMutex<()>,
    model: AiMaskModel,
) -> Result<Arc<AiModels>> {
    let is_downloaded = |models: &AiModels| models.downloaded.lock().unwrap().contains(&model);

    if let Some(models) = cached_ai_state(ai_state_mutex, |state| state.models.clone())
        && is_downloaded(&models)
    {
        return Ok(models);
    }

    let _guard = ai_init_lock.lock().await;

    let models = match cached_ai_state(ai_state_mutex, |state| state.models.clone()) {
        Some(models) => models,
        None => {
            let _ = ort::init().with_name("AI").commit();
            crate::register_exit_handler();

            let models = Arc::new(AiModels {
                models_dir: get_models_dir(app_handle)?,
                use_gpu: gpu_acceleration_enabled(app_handle),
                downloaded: Mutex::new(HashSet::new()),
                sam_encoder: Mutex::new(None),
                sam_decoder: Mutex::new(None),
                birefnet: Mutex::new(None),
                esam3_image_encoder: Mutex::new(None),
                esam3_decoder: Mutex::new(None),
                esam3_text_encoder: Mutex::new(None),
                esam3_sky_prompt: Mutex::new(None),
                esam3_face_prompt: Mutex::new(None),
                clip_tokenizer: Mutex::new(None),
                face_landmarks: Mutex::new(None),
                face_segmenter: Mutex::new(None),
                depth_anything: Mutex::new(None),
            });
            ai_state_mutex
                .lock()
                .unwrap()
                .get_or_insert_with(AiState::default)
                .models = Some(models.clone());
            models
        }
    };

    if !is_downloaded(&models) {
        for asset in model.assets(models.gpu_inference()) {
            download_and_verify_model(app_handle, &models.models_dir, asset).await?;
        }
        models.downloaded.lock().unwrap().insert(model);
    }

    Ok(models)
}

pub async fn get_or_init_denoise_model(
    app_handle: &tauri::AppHandle,
    ai_state_mutex: &Mutex<Option<AiState>>,
    ai_init_lock: &TokioMutex<()>,
) -> Result<Arc<DenoiseSession>> {
    if let Some(denoise_model) =
        cached_ai_state(ai_state_mutex, |state| state.denoise_model.clone())
    {
        return Ok(denoise_model);
    }

    let _guard = ai_init_lock.lock().await;

    if let Some(denoise_model) =
        cached_ai_state(ai_state_mutex, |state| state.denoise_model.clone())
    {
        return Ok(denoise_model);
    }

    let models_dir = get_models_dir(app_handle)?;

    // OpenHarmony NPU path (CANN .om first, then MindSpore Lite .ms), only
    // when the user selected the NPU backend. Any problem falls back to ORT.
    #[cfg(target_env = "ohos")]
    {
        let npu_selected = crate::app::settings::load_settings(app_handle.clone())
            .unwrap_or_default()
            .ai_denoise_backend
            .as_deref()
            .is_some_and(|backend| backend == "npu");
        if npu_selected {
            // Prefer a side-loaded/downloaded .om; otherwise extract the copy
            // bundled into a release HAP (build-ohos.ps1 -Release).
            let om_path = match provision_bundled_model(app_handle, DENOISE_OM_FILENAME) {
                Ok(Some(path)) => path,
                Ok(None) => models_dir.join(DENOISE_OM_FILENAME),
                Err(e) => {
                    log::warn!(
                        "NPU mode: bundled {} extraction failed ({e}), trying .ms/ORT fallback.",
                        DENOISE_OM_FILENAME
                    );
                    models_dir.join(DENOISE_OM_FILENAME)
                }
            };
            if om_path.exists() {
                match crate::ohos_integration::om::OmModel::new(&om_path, OM_PERF_MODE_CNN) {
                    Ok(model) if denoise_om_contract_matches(&model) => {
                        log::info!("NPU mode: using CANN .om denoise model.");
                        let denoise_model = Arc::new(DenoiseSession::Cann(Mutex::new(model)));
                        crate::register_exit_handler();
                        ai_state_mutex
                            .lock()
                            .unwrap()
                            .get_or_insert_with(AiState::default)
                            .denoise_model = Some(denoise_model.clone());
                        return Ok(denoise_model);
                    }
                    Ok(_) => {
                        log::warn!(
                            "NPU mode: {} IO contract mismatch, trying .ms/ORT fallback.",
                            DENOISE_OM_FILENAME
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "NPU mode: .om denoise init failed ({e}), trying .ms/ORT fallback."
                        );
                    }
                }
            }

            match try_provision_ms_model(app_handle, &models_dir).await {
                Ok(Some(ms_path)) => match crate::ohos_integration::ms_lite::MsModel::new(&ms_path)
                {
                    Ok(ms_model) => {
                        log::info!("NPU mode: using MindSpore Lite .ms denoise model.");
                        let denoise_model = Arc::new(DenoiseSession::Nnrt(Mutex::new(ms_model)));
                        crate::register_exit_handler();
                        ai_state_mutex
                            .lock()
                            .unwrap()
                            .get_or_insert_with(AiState::default)
                            .denoise_model = Some(denoise_model.clone());
                        return Ok(denoise_model);
                    }
                    Err(e) => {
                        log::warn!(
                            "NPU mode: MsModel build failed ({e}), falling back to ORT CPU."
                        );
                    }
                },
                _ => {
                    log::warn!("NPU mode: .ms model not available, falling back to ORT CPU.");
                }
            }
        }
    }

    download_and_verify_model(app_handle, &models_dir, &DENOISE_ASSET).await?;

    ensure_ort_available().map_err(anyhow::Error::msg)?;
    let source = ModelSource::resolve(DENOISE_FILENAME, &models_dir);
    let session = build_session(
        &source,
        SessionKind::Persistent,
        gpu_acceleration_enabled(app_handle),
        Some(denoise_crop_as_slice),
    )?;
    let denoise_model = Arc::new(DenoiseSession::Ort(Mutex::new(session)));

    crate::register_exit_handler();

    ai_state_mutex
        .lock()
        .unwrap()
        .get_or_insert_with(AiState::default)
        .denoise_model = Some(denoise_model.clone());

    Ok(denoise_model)
}

/// Validate the CANN (.om) denoise model's IO contract: one f32 input
/// `[1,3,504,504]` and one f32 output `[1,3,504,504]` (the app's
/// nind_denoise_utnet_684 lineage after Pad→Slice surgery -> omg; 504 is
/// the fixed tile size all TileParams variants use).
#[cfg(target_env = "ohos")]
fn denoise_om_contract_matches(model: &crate::ohos_integration::om::OmModel) -> bool {
    let expected_bytes = 3 * 504 * 504 * std::mem::size_of::<f32>();
    model.input_count() == 1
        && model.output_count() == 1
        && model
            .input_info(0)
            .is_some_and(|i| i.dims == [1, 3, 504, 504] && i.byte_size == expected_bytes)
        && model
            .output_info(0)
            .is_some_and(|o| o.dims == [1, 3, 504, 504] && o.byte_size == expected_bytes)
}

/// The RapidRes `.om` is converted from the same ONNX with its tile input
/// pinned to the upscale tile size (544x544, overlap 16) and must upscale
/// exactly 2x (docs/RAPIDRES_NPU_FEASIBILITY.md §1).
#[cfg(target_env = "ohos")]
fn upscale_om_contract_matches(model: &crate::ohos_integration::om::OmModel) -> bool {
    const TILE: usize = crate::ai::tiling::UPSCALE_TILE_SIZE;
    let out = TILE * 2;
    let in_bytes = 3 * TILE * TILE * std::mem::size_of::<f32>();
    let out_bytes = 3 * out * out * std::mem::size_of::<f32>();
    let tile = TILE as i32;
    let out_dim = out as i32;
    model.input_count() == 1
        && model.output_count() == 1
        && model
            .input_info(0)
            .is_some_and(|i| i.dims == [1, 3, tile, tile] && i.byte_size == in_bytes)
        && model
            .output_info(0)
            .is_some_and(|o| o.dims == [1, 3, out_dim, out_dim] && o.byte_size == out_bytes)
}

/// Provision the MindSpore Lite denoise model: prefer a copy bundled in the
/// HAP rawfile set (release builds), otherwise download/verify it. Either way
/// the pinned SHA256 is enforced. Any failure (bad hash, download error, IO
/// error) results in `Ok(None)`, causing the caller to fall back to the ORT CPU
/// path.
#[cfg(target_env = "ohos")]
async fn try_provision_ms_model(
    app_handle: &tauri::AppHandle,
    models_dir: &Path,
) -> Result<Option<PathBuf>> {
    match provision_bundled_model(app_handle, DENOISE_MS_FILENAME) {
        Ok(Some(path)) => {
            if verify_sha256(&path, DENOISE_MS_SHA256)? {
                return Ok(Some(path));
            }
            log::warn!(
                "NPU mode: bundled {} failed sha256 verification, removing and downloading.",
                path.display()
            );
            let _ = fs::remove_file(&path);
        }
        Ok(None) => {}
        Err(e) => log::warn!("NPU mode: bundled .ms extraction failed ({e}), downloading."),
    }

    let asset = ModelAsset {
        filename: DENOISE_MS_FILENAME,
        url: DENOISE_MS_URL,
        sha256: Some(DENOISE_MS_SHA256),
        name: "NIND Denoise .ms",
    };
    match download_and_verify_model(app_handle, models_dir, &asset).await {
        Ok(()) => Ok(Some(models_dir.join(DENOISE_MS_FILENAME))),
        Err(e) => {
            log::warn!("NPU model download failed ({e}), falling back to ORT.");
            Ok(None)
        }
    }
}

pub async fn get_or_init_upscale_model(
    app_handle: &tauri::AppHandle,
    ai_state_mutex: &Mutex<Option<AiState>>,
    ai_init_lock: &TokioMutex<()>,
) -> Result<Arc<UpscaleSession>> {
    if let Some(upscale_model) =
        cached_ai_state(ai_state_mutex, |state| state.upscale_model.clone())
    {
        return Ok(upscale_model);
    }

    let _guard = ai_init_lock.lock().await;

    if let Some(upscale_model) =
        cached_ai_state(ai_state_mutex, |state| state.upscale_model.clone())
    {
        return Ok(upscale_model);
    }

    let models_dir = get_models_dir(app_handle)?;

    // OpenHarmony NPU path, mirroring the denoise selection: the shared NPU
    // backend setting gates it, a side-loaded SoC-specific `.om` provides the
    // graph (converted from the same ONNX with its tile input pinned to
    // 544x544 — docs/RAPIDRES_NPU_FEASIBILITY.md §6). Any problem falls
    // back to ORT.
    #[cfg(target_env = "ohos")]
    {
        let npu_selected = crate::app::settings::load_settings(app_handle.clone())
            .unwrap_or_default()
            .ai_denoise_backend
            .as_deref()
            .is_some_and(|backend| backend == "npu");
        if npu_selected {
            let om_path = models_dir.join(UPSCALE_OM_FILENAME);
            if om_path.exists() {
                match crate::ohos_integration::om::OmModel::new(&om_path, OM_PERF_MODE_CNN) {
                    Ok(model) if upscale_om_contract_matches(&model) => {
                        log::info!("NPU mode: using CANN .om upscale model.");
                        let upscale_model = Arc::new(UpscaleSession::Cann(Mutex::new(model)));
                        crate::register_exit_handler();
                        ai_state_mutex
                            .lock()
                            .unwrap()
                            .get_or_insert_with(AiState::default)
                            .upscale_model = Some(upscale_model.clone());
                        return Ok(upscale_model);
                    }
                    Ok(_) => {
                        log::warn!(
                            "NPU mode: {} IO contract mismatch, trying .onnx/ORT fallback.",
                            UPSCALE_OM_FILENAME
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "NPU mode: .om upscale init failed ({e}), trying .onnx/ORT fallback."
                        );
                    }
                }
            } else {
                log::warn!(
                    "NPU mode: no {} in the models dir, using the ORT upscale model.",
                    UPSCALE_OM_FILENAME
                );
            }
        }
    }

    download_and_verify_model(app_handle, &models_dir, &UPSCALE_ASSET).await?;

    let _ = ort::init().with_name("AI-Upscale").commit();
    let tile = crate::ai::tiling::UPSCALE_TILE_SIZE as i64;
    let session = build_session_with_dims(
        &ModelSource::resolve(UPSCALE_FILENAME, &models_dir),
        SessionKind::Persistent,
        gpu_acceleration_enabled(app_handle),
        None,
        &UPSCALE_SPATIAL_DIMS.map(|dim| (dim, tile)),
    )?;
    let upscale_model = Arc::new(UpscaleSession::Ort(Mutex::new(session)));

    crate::register_exit_handler();

    ai_state_mutex
        .lock()
        .unwrap()
        .get_or_insert_with(AiState::default)
        .upscale_model = Some(upscale_model.clone());

    Ok(upscale_model)
}

pub async fn get_or_init_clip_models(
    app_handle: &tauri::AppHandle,
    ai_state_mutex: &Mutex<Option<AiState>>,
    ai_init_lock: &TokioMutex<()>,
) -> Result<Arc<ClipModels>> {
    if let Some(clip_models) = cached_ai_state(ai_state_mutex, |state| state.clip_models.clone()) {
        return Ok(clip_models);
    }

    let _guard = ai_init_lock.lock().await;

    if let Some(clip_models) = cached_ai_state(ai_state_mutex, |state| state.clip_models.clone()) {
        return Ok(clip_models);
    }

    let models_dir = get_models_dir(app_handle)?;

    download_and_verify_model(app_handle, &models_dir, &CLIP_MODEL_ASSET).await?;
    download_and_verify_model(app_handle, &models_dir, &CLIP_TOKENIZER_ASSET).await?;

    let _ = ort::init().with_name("AI-Tagging").commit();
    let source = ModelSource::resolve(CLIP_MODEL_FILENAME, &models_dir);
    let model = Mutex::new(build_session(
        &source,
        SessionKind::Persistent,
        gpu_allowed_on_platform() && gpu_acceleration_enabled(app_handle),
        None,
    )?);
    // The tokenizer json is tiny; prefer the app-data file, fall back to the
    // HAP bundle, and only then error out (the ensure step above guarantees
    // one of the two exists).
    let tokenizer_bytes = fs::read(models_dir.join(CLIP_TOKENIZER_FILENAME))
        .ok()
        .or_else(|| read_bundled_model(CLIP_TOKENIZER_FILENAME));
    let tokenizer = match tokenizer_bytes {
        Some(bytes) => Tokenizer::from_bytes(&bytes).map_err(|e| anyhow::anyhow!(e.to_string()))?,
        None => {
            return Err(anyhow::anyhow!(
                "CLIP tokenizer {} not found in models dir or HAP bundle",
                CLIP_TOKENIZER_FILENAME
            ));
        }
    };

    crate::register_exit_handler();

    let clip_models = Arc::new(ClipModels { model, tokenizer });

    ai_state_mutex
        .lock()
        .unwrap()
        .get_or_insert_with(AiState::default)
        .clip_models = Some(clip_models.clone());

    Ok(clip_models)
}

pub async fn get_or_init_lama_model(
    app_handle: &tauri::AppHandle,
    ai_state_mutex: &Mutex<Option<AiState>>,
    ai_init_lock: &TokioMutex<()>,
) -> Result<Arc<Mutex<Session>>> {
    if let Some(lama_model) = cached_ai_state(ai_state_mutex, |state| state.lama_model.clone()) {
        return Ok(lama_model);
    }

    let _guard = ai_init_lock.lock().await;

    if let Some(lama_model) = cached_ai_state(ai_state_mutex, |state| state.lama_model.clone()) {
        return Ok(lama_model);
    }

    let models_dir = get_models_dir(app_handle)?;
    download_and_verify_model(app_handle, &models_dir, &LAMA_ASSET).await?;

    let _ = ort::init().with_name("AI-Inpainting").commit();
    let source = ModelSource::resolve(LAMA_FILENAME, &models_dir);
    let session = build_session(&source, SessionKind::OneShot, false, None)?;
    let lama_model = Arc::new(Mutex::new(session));

    crate::register_exit_handler();

    ai_state_mutex
        .lock()
        .unwrap()
        .get_or_insert_with(AiState::default)
        .lama_model = Some(lama_model.clone());

    Ok(lama_model)
}

pub async fn get_or_init_normal_model(
    app_handle: &tauri::AppHandle,
    ai_state_mutex: &Mutex<Option<AiState>>,
    ai_init_lock: &TokioMutex<()>,
) -> Result<Arc<Mutex<Session>>> {
    if let Some(normal_model) = cached_ai_state(ai_state_mutex, |state| state.normal_model.clone())
    {
        return Ok(normal_model);
    }

    let _guard = ai_init_lock.lock().await;

    if let Some(normal_model) = cached_ai_state(ai_state_mutex, |state| state.normal_model.clone())
    {
        return Ok(normal_model);
    }

    let models_dir = get_models_dir(app_handle)?;
    download_and_verify_model(app_handle, &models_dir, &NORMAL_ASSET).await?;

    let _ = ort::init().with_name("AI-Normal").commit();
    let source = ModelSource::resolve(NORMAL_FILENAME, &models_dir);
    let session = build_session(
        &source,
        SessionKind::OneShot,
        gpu_allowed_on_platform() && gpu_acceleration_enabled(app_handle),
        None,
    )?;
    let normal_model = Arc::new(Mutex::new(session));

    crate::register_exit_handler();

    ai_state_mutex
        .lock()
        .unwrap()
        .get_or_insert_with(AiState::default)
        .normal_model = Some(normal_model.clone());

    Ok(normal_model)
}

const ESAM3_PROMPT_TOKENS: usize = 16;

const ESAM3_EMBED_DIM: usize = 256;

pub struct Esam3Prompt {
    pub features: Array<f32, IxDyn>,
    pub padding: Array<f32, IxDyn>,
}

/// Denoise model backend. ORT is the default on every platform; on
/// OpenHarmony the NPU backends are selected through the AI denoise
/// backend setting (docs/AI_DENOISE_HW_ACCELERATION.md).
pub enum DenoiseSession {
    Ort(Mutex<Session>),
    /// MindSpore Lite NNRT (.ms model).
    #[cfg(target_env = "ohos")]
    Nnrt(Mutex<crate::ohos_integration::ms_lite::MsModel>),
    /// CANN/HIAI (.om model).
    #[cfg(target_env = "ohos")]
    Cann(Mutex<crate::ohos_integration::om::OmModel>),
}

/// RapidRes upscale model backend — the same NPU selection story as
/// `DenoiseSession` (the `ai_denoise_backend` setting governs the whole NPU
/// pipeline). ORT is the default; a side-loaded
/// `2xBHI_small_span_rapidraw.om` next to the ONNX in the models dir runs
/// on the CANN/HIAI NPU instead.
pub enum UpscaleSession {
    Ort(Mutex<Session>),
    /// CANN/HIAI (.om model).
    #[cfg(target_env = "ohos")]
    Cann(Mutex<crate::ohos_integration::om::OmModel>),
}

#[derive(Default)]
pub struct AiState {
    pub models: Option<Arc<AiModels>>,
    pub denoise_model: Option<Arc<DenoiseSession>>,
    pub upscale_model: Option<Arc<UpscaleSession>>,
    pub clip_models: Option<Arc<ClipModels>>,
    pub lama_model: Option<Arc<Mutex<Session>>>,
    pub normal_model: Option<Arc<Mutex<Session>>>,
    pub embeddings: Option<ImageEmbeddings>,
    pub esam3_features: Option<Arc<Esam3Features>>,
    pub face_analysis: Option<Arc<FaceAnalysis>>,
    pub depth_map: Option<CachedDepthMap>,
}

impl AiState {
    pub fn release_sessions(&mut self) {
        self.models = None;
        self.denoise_model = None;
        self.upscale_model = None;
        self.clip_models = None;
        self.lama_model = None;
        self.normal_model = None;
    }
}

fn cached_ai_state<T>(
    ai_state_mutex: &Mutex<Option<AiState>>,
    get: impl Fn(&AiState) -> Option<T>,
) -> Option<T> {
    let mut ai_state = ai_state_mutex.lock().unwrap();
    if GPU_STATUS
        .compare_exchange(
            GPU_FAILED,
            GPU_DISABLED,
            Ordering::AcqRel,
            Ordering::Acquire,
        )
        .is_ok()
        && let Some(ai_state) = ai_state.as_mut()
    {
        ai_state.release_sessions();
    }
    ai_state.as_ref().and_then(get)
}

fn denoise_crop_as_slice() -> ort::Result<(Model, Vec<Output>)> {
    let output_type = ValueType::Tensor {
        ty: TensorElementType::Float32,
        shape: Shape::new([1, 3, -1, -1]),
        dimension_symbols: SymbolicDimensions::empty(4),
    };

    let mut graph = Graph::new()?;
    graph.add_initializer("crop_starts", i64_initializer(&[2, 2])?, false)?;
    graph.add_initializer("crop_ends", i64_initializer(&[-2, -2])?, false)?;
    graph.add_initializer("crop_axes", i64_initializer(&[2, 3])?, false)?;
    graph.add_node(Node::new(
        "Slice",
        ONNX_DOMAIN,
        "crop_slice",
        ["conv2d_9", "crop_starts", "crop_ends", "crop_axes"],
        ["denoised"],
        [],
    )?)?;
    graph.set_outputs([Outlet::new("denoised", &output_type)?])?;

    let mut model = Model::new([Opset::new(ONNX_DOMAIN, 18)?])?;
    model.add_graph(graph)?;
    Ok((
        model,
        vec![Output {
            name: "denoised".to_string(),
            output_type,
        }],
    ))
}
