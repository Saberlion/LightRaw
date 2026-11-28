#[cfg(not(target_os = "android"))]
use crate::app::settings::load_settings;
use crate::app::state::{AppState, PreviewTarget};
#[cfg(target_os = "android")]
use crate::render::gpu::display::PlatformSurface;
#[cfg(all(target_os = "linux", not(target_env = "ohos")))]
use crate::render::gpu::display::WGPU_DISPLAY_FAILED;
use crate::render::gpu::display::WgpuDisplay;
#[cfg(not(target_os = "android"))]
use crate::render::gpu::display::{build_wgpu_display, surface_for_window};
use crate::render::gpu::pipelines::GpuPipelines;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tauri::Manager;

pub fn current_gpu_context(state: &AppState) -> Option<GpuContext> {
    state
        .gpu_context
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Zero-driver-touch GPU status for the frontend and diagnostics: reads
/// atomics and (via try_lock) the context slot only. Never creates a context
/// and never calls into the GPU driver, so it is safe to poll at any time —
/// including during the OHOS first-frame window, where touching the driver is
/// exactly what must not happen. try_lock keeps a concurrent init from
/// blocking the poll: "not ready yet" is the honest answer then.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuStatus {
    /// Platform GPU policy: "auto" (desktop) | "vulkan-only" (OHOS devices) |
    /// "blocked" (x86_64 OHOS — the DevEco emulator ICD aborts the process
    /// during the cooperative-matrix probe, see the guard in
    /// get_or_init_gpu_context).
    pub policy: &'static str,
    /// Whether the first-frame window is open (always true off OHOS devices).
    pub window_open: bool,
    /// Whether a live GPU context already exists.
    pub context_ready: bool,
    /// Whether the device was lost and the next GPU call re-creates the context.
    pub device_lost: bool,
}

#[tauri::command]
pub fn gpu_status(state: tauri::State<'_, AppState>) -> GpuStatus {
    GpuStatus {
        policy: if cfg!(all(target_env = "ohos", target_arch = "x86_64")) {
            "blocked"
        } else if cfg!(target_env = "ohos") {
            "vulkan-only"
        } else {
            "auto"
        },
        window_open: gpu_window_open(),
        context_ready: state
            .gpu_context
            .try_lock()
            .map(|context| context.is_some())
            .unwrap_or(false),
        device_lost: GPU_DEVICE_LOST.load(Ordering::Acquire),
    }
}

static GPU_DEVICE_LOST: AtomicBool = AtomicBool::new(false);

/// First-frame window for OHOS GPU init (BUG-2026-11-25-29 follow-up): the
/// warmup-only gate left every other GPU entry point (thumbnail/sidecar
/// workers, LUT/preset previews, editor commands — ~20 get_or_init_gpu_context
/// call sites) relying on timing luck against the ArkWeb compositor's
/// startup burst inside the Maleoon driver. The gate sits at the choke point
/// instead: the first GPU init of the process waits (bounded) for the
/// frontend's `webview_first_frame_stable` signal plus a settle delay, no
/// matter which thread gets there first; every later caller passes straight
/// through. Blocking, not cancelling — callers are background workers and
/// request handlers that would rather wait 3.5s than race the driver.
#[cfg(all(target_env = "ohos", not(target_arch = "x86_64")))]
static OHOS_GPU_WINDOW_OPEN: AtomicBool = AtomicBool::new(false);

/// Opens the OHOS GPU window. Idempotent; free once open. The bound and the
/// fallback semantics mirror the warmup gate: a missing signal only delays
/// the init, it never cancels it (a slow frontend degrades to a late warmup).
#[cfg(all(target_env = "ohos", not(target_arch = "x86_64")))]
fn wait_for_gpu_window() {
    if OHOS_GPU_WINDOW_OPEN.load(Ordering::Acquire) {
        return;
    }
    crate::render::gpu::processor::wait_for_webview_first_frame(std::time::Duration::from_secs(3));
    // Let the compositor settle past the first presented frames (entrance
    // animations, tile rasterization) — the same margin the warmup used.
    std::thread::sleep(std::time::Duration::from_millis(500));
    OHOS_GPU_WINDOW_OPEN.store(true, Ordering::Release);
}

/// Window state for gpu_status. Always true where no gate exists; the x86_64
/// emulator reports policy "blocked" instead (its guard never lets a GPU
/// context exist, the window concept does not apply there).
#[cfg(all(target_env = "ohos", not(target_arch = "x86_64")))]
pub fn gpu_window_open() -> bool {
    OHOS_GPU_WINDOW_OPEN.load(Ordering::Acquire)
}
#[cfg(not(all(target_env = "ohos", not(target_arch = "x86_64"))))]
pub fn gpu_window_open() -> bool {
    true
}

static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Serializes every GPU object-creation burst (pipeline / bind-group-layout
/// compilation - a long run of vkCreate* calls) across ALL threads.
///
/// BUG-2026-11-28-25/26 (Maleoon v200/B322): the driver's internal
/// descriptor-layout container is not thread-safe. The background
/// compute-warming thread spawned at context creation used to run with no
/// lock, racing the pipeline-warmup thread and the main-path processor
/// creation; the torn writes corrupt a driver-internal pointer (fault x1
/// holds packed non-pointer data) and the next vkCreateDescriptorSetLayout
/// dereferences it - SIGSEGV in libmaleoon/hvgr with the register-forensics
/// signatures from TEST-2026-11-28. Every creation site takes this mutex:
/// background warmer, warmup, output.rs processor creation, shader_check.
pub static GPU_OBJECT_CREATION: Mutex<()> = Mutex::new(());

pub fn register_app_handle(app_handle: &tauri::AppHandle) {
    let _ = APP_HANDLE.set(app_handle.clone());
}

pub fn app_handle() -> Option<&'static tauri::AppHandle> {
    APP_HANDLE.get()
}

pub fn shared_gpu_context() -> Result<GpuContext, String> {
    let app_handle = app_handle().ok_or("GPU context requested before app setup")?;
    get_or_init_gpu_context(&app_handle.state::<AppState>(), app_handle)
}

#[derive(Clone)]
pub struct DecodeGpu {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub limits: wgpu::Limits,
    pub adapter: String,
}

struct DecodeSlot {
    owner: Arc<wgpu::Device>,
    gpu: DecodeGpu,
    lean: bool,
}

static DECODE_GPU: Mutex<Option<DecodeSlot>> = Mutex::new(None);
static DECODE_DEVICE_LOST: AtomicBool = AtomicBool::new(false);

pub fn adapter_label(adapter: &wgpu::Adapter) -> String {
    let info = adapter.get_info();
    format!(
        "{} ({:?}, {} {})",
        info.name, info.backend, info.driver, info.driver_info
    )
}

pub fn decode_gpu_context() -> Result<DecodeGpu, String> {
    let context = shared_gpu_context()?;
    if context.shares_one_gl_context() {
        return Err("the GL backend cannot run decoding next to rendering".into());
    }
    Ok(decode_gpu_for(&context))
}

pub fn release_decode_gpu(failed: &Arc<wgpu::Device>) -> bool {
    let mut slot = DECODE_GPU.lock().unwrap_or_else(|e| e.into_inner());
    let released = slot.as_ref().is_some_and(|slot| {
        !slot.lean
            && Arc::ptr_eq(&slot.gpu.device, failed)
            && !Arc::ptr_eq(&slot.gpu.device, &slot.owner)
    });
    if released {
        *slot = None;
    }
    released
}

fn decode_gpu_for(context: &GpuContext) -> DecodeGpu {
    let mut slot = DECODE_GPU.lock().unwrap_or_else(|e| e.into_inner());
    if DECODE_DEVICE_LOST.swap(false, Ordering::AcqRel) {
        *slot = None;
    }
    if let Some(existing) = slot.as_ref()
        && Arc::ptr_eq(&existing.owner, &context.device)
    {
        return existing.gpu.clone();
    }
    let adapter = adapter_label(&context.adapter);
    let lean = crate::decode::gpu_fallback::has_failed(&adapter);
    let opened = open_device(
        &context.adapter,
        "Decode Device",
        wgpu::Features::empty(),
        &context.limits,
        if lean {
            wgpu::MemoryHints::MemoryUsage
        } else {
            wgpu::MemoryHints::Performance
        },
        &DECODE_DEVICE_LOST,
    );
    let (device, queue) = opened.unwrap_or_else(|e| {
        log::warn!("Decoding on the display queue: {}", e);
        (Arc::clone(&context.device), Arc::clone(&context.queue))
    });
    let gpu = DecodeGpu {
        device,
        queue,
        limits: context.limits.clone(),
        adapter,
    };
    *slot = Some(DecodeSlot {
        owner: Arc::clone(&context.device),
        gpu: gpu.clone(),
        lean,
    });
    gpu
}

fn open_device(
    adapter: &wgpu::Adapter,
    label: &'static str,
    required_features: wgpu::Features,
    limits: &wgpu::Limits,
    memory_hints: wgpu::MemoryHints,
    lost: &'static AtomicBool,
) -> Result<(Arc<wgpu::Device>, Arc<wgpu::Queue>), String> {
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some(label),
        required_features,
        required_limits: limits.clone(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints,
        trace: wgpu::Trace::Off,
    }))
    .map_err(|e| e.to_string())?;
    device.on_uncaptured_error(Arc::new(move |err: wgpu::Error| {
        log::error!("[wgpu-error] {}: {}", label, err);
    }));
    device.set_device_lost_callback(move |reason, message| {
        if reason == wgpu::DeviceLostReason::Unknown {
            log::error!("{} lost: {}", label, message);
            lost.store(true, Ordering::Release);
        }
    });
    Ok((Arc::new(device), Arc::new(queue)))
}

pub fn get_or_init_gpu_context(
    state: &tauri::State<AppState>,
    #[cfg_attr(target_os = "android", allow(unused_variables))] app_handle: &tauri::AppHandle,
) -> Result<GpuContext, String> {
    // OHOS first-frame gate (see wait_for_gpu_window): throttle the FIRST GPU
    // init of the process behind the webview's stable first frames, before
    // the context lock is taken, so context peeks (gpu_status /
    // current_gpu_context) never block on the wait. Excluded on x86_64: the
    // emulator guard below returns before the GPU is ever touched, and the
    // wait would be dead time there.
    #[cfg(all(target_env = "ohos", not(target_arch = "x86_64")))]
    wait_for_gpu_window();

    let mut context_lock = match state.gpu_context.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            log::warn!(
                "GPU context lock was poisoned (previous crash). Wiping state to self-heal."
            );
            let mut guard = poisoned.into_inner();
            *guard = None;
            guard
        }
    };
    #[cfg(all(target_os = "linux", not(target_env = "ohos")))]
    let mut reused_wayland = None;
    if GPU_DEVICE_LOST.load(Ordering::Acquire) {
        log::warn!("Recreating GPU context after the device was lost.");
        // BUG-2026-11-28-33 (v200/Mate 80): the device-lost recreation path
        // used to create a new context with NO first-frame gate — the
        // OHOS_GPU_WINDOW_OPEN atomic was already set by the (successful)
        // warmup, so wait_for_gpu_window() returned instantly and the new
        // adapter enumeration raced the ArkWeb compositor on the v200
        // driver (SIGSEGV in libmaleoon_v200.so, warmup thread, after
        // "[pipeline-warmup] completed" logged + second "Using GPU
        // adapter" line). Reset the gate so the recreation goes through
        // the same first-frame wait + settle as the first creation.
        #[cfg(all(target_env = "ohos", not(target_arch = "x86_64")))]
        OHOS_GPU_WINDOW_OPEN.store(false, Ordering::Release);
        if let Some(context) = context_lock.take() {
            #[cfg(all(target_os = "linux", not(target_env = "ohos")))]
            {
                reused_wayland = context
                    .display
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                    .and_then(|mut display| display.wayland.take());
            }
            *context.display.lock().unwrap_or_else(|e| e.into_inner()) = None;
            *context
                .viewer_display
                .lock()
                .unwrap_or_else(|e| e.into_inner()) = None;
        }
        *lock_or_reset(&state.gpu_processor, "GPU processor") = None;
        for target in PreviewTarget::ALL {
            *lock_or_reset(state.gpu_image_cache_for(target), "GPU image cache") = None;
        }
        GPU_DEVICE_LOST.store(false, Ordering::Release);
    }
    if let Some(context) = &*context_lock {
        return Ok(context.clone());
    }

    #[allow(unused_mut)]
    let mut instance_desc = wgpu::InstanceDescriptor::new_without_display_handle_from_env();

    // Maleoon driver bug forensics (BUG-2026-11-28-26 / BUG-2026-11-25-29):
    // the v200 (Mate 80) and B322 (GEM-W40) drivers SEGV inside their
    // debug-utils object-name handling - wgpu's HAL labels ("(wgpu
    // internal) zero init buffer", ...) end up dereferenced as pointers
    // (fault registers x6/x7 hold "(wgpu in" + "ternal) ", the fault
    // address itself is built from label bytes). hal_label only strips
    // these when DISCARD_HAL_LABELS is set, and the flag default never
    // sets it, so every OHOS build has been handing debug-utils names to
    // the driver. Discard them: labels are a debugging nicety, the driver
    // treating them as pointers is a crash. NOTE: the B322 SIGSEGV was
    // proven to persist WITH labels discarded (constant fault address,
    // registers zeroed) - this flag is defense-in-depth for the v200
    // variant (whose fault address really is label bytes), not a fix for
    // B322 (that one is the driver blocklist below).
    #[cfg(target_env = "ohos")]
    {
        instance_desc.flags |= wgpu::InstanceFlags::DISCARD_HAL_LABELS;
    }

    #[cfg(target_os = "windows")]
    if std::env::var("WGPU_BACKEND").is_err() {
        instance_desc.backends = wgpu::Backends::PRIMARY;
    }

    // The DevEco x86_64 emulator is the only x86_64 OpenHarmony environment,
    // and its Vulkan ICD is non-conformant: it advertises
    // VK_KHR_cooperative_matrix without implementing the entry point, so
    // wgpu-hal's capability probe reaches the loader terminator, which calls
    // abort() and kills the process (SIGABRT, cppcrash pair 2026-11-27
    // 23:09:02/23:09:26: "ICD associated with VkPhysicalDevice does not
    // support GetPhysicalDeviceCooperativeMatrixPropertiesKHR"). All real
    // OHOS devices are ARM, so fail context creation up front and let every
    // GPU call site take its existing no-adapter degradation path — porting
    // doc 6.16 already declares the emulator non-rendering; without this the
    // app dies ~6s after every launch on the DevEco 26 image.
    #[cfg(all(target_env = "ohos", target_arch = "x86_64"))]
    {
        return Err(
            "GPU unavailable on x86_64 OHOS: the DevEco emulator's Vulkan ICD \
             aborts the process during the cooperative-matrix probe; \
             running in degraded (no-GPU) mode"
                .into(),
        );
    }

    // OpenHarmony renders through Vulkan and nothing else. There is no GLES
    // fallback any more: the translation layer has smashed the stack inside the
    // readback path (libhvgr_v210.so, 2026-10-05 faultlog cppcrash-24077) and
    // deadlocks on the adapter-context lock when preview and thumbnail workers
    // render concurrently (GEM-W40, wgpu-hal gles/device.rs:1594). Vulkan needs
    // the ash fork so it looks for `libvulkan.so` instead of `libvulkan.so.1`
    // (porting doc 6.9, `[patch.crates-io]`). The settings backend choice is
    // ignored on this platform (lib.rs pins it to `vulkan`).
    #[cfg(target_env = "ohos")]
    {
        instance_desc.backends = wgpu::Backends::VULKAN;
    }

    crate::render::gpu::fallback::begin_attempt(
        &std::env::var("WGPU_BACKEND").unwrap_or_else(|_| "auto".to_string()),
    );

    let instance = wgpu::Instance::new(instance_desc);

    #[cfg(not(target_os = "android"))]
    let surface_opt = {
        let settings = load_settings(app_handle.clone()).unwrap_or_default();
        if settings.wgpu_renderer_enabled() {
            if let Some(window) = app_handle.get_webview_window("main") {
                #[cfg(all(target_os = "linux", not(target_env = "ohos")))]
                let created = match reused_wayland {
                    Some(wayland) => wayland
                        .create_surface(&instance)
                        .map(|surface| (surface, Some(wayland))),
                    None => surface_for_window(&instance, &window),
                };
                #[cfg(not(all(target_os = "linux", not(target_env = "ohos"))))]
                let created = surface_for_window(&instance, &window);
                match created {
                    Ok(surface) => Some(surface),
                    Err(e) => {
                        log::warn!(
                            "Failed to create surface, falling back to compute-only: {}",
                            e
                        );
                        #[cfg(all(target_os = "linux", not(target_env = "ohos")))]
                        WGPU_DISPLAY_FAILED.store(true, Ordering::Release);
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        }
    };

    #[cfg(target_os = "android")]
    let surface_opt: Option<(wgpu::Surface, PlatformSurface)> = None;

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: surface_opt.as_ref().map(|(surface, _)| surface),
        ..Default::default()
    }))
    .map_err(|e| {
        crate::render::gpu::fallback::abandon_attempt();
        format!("Failed to find a wgpu adapter: {}", e)
    })?;

    let adapter_info = adapter.get_info();
    log::info!(
        "Using GPU adapter: {} ({:?}, {:?}, driver: {} {})",
        adapter_info.name,
        adapter_info.backend,
        adapter_info.device_type,
        adapter_info.driver,
        adapter_info.driver_info
    );
    // Maleoon B322 (GEM-W40 / 935A) driver, BUG-2026-11-25/26: vkCreateDescriptorSetLayout
    // SIGSEGVs when object-creation bursts from two threads interleave - the
    // driver's internal descriptor-layout container is not thread-safe
    // (forward-forensics: faulting instruction `ldp [x1,#-16]` on an
    // intrusive-list node whose pointer field holds packed non-pointer data).
    // VERIFICATION BUILD 2026-11-28: root-caused to the UNLOCKED background
    // compute warmer racing the warmup/main creation paths; all creation
    // sites now serialize through GPU_OBJECT_CREATION and the blocklist is
    // LIFTED. If this build still crashes, restore the Err return from git
    // history (2a15998b) and keep the CPU degradation.
    crate::render::gpu::fallback::begin_attempt(crate::render::gpu::fallback::backend_name(
        adapter_info.backend,
    ));

    let mut required_features = wgpu::Features::empty();
    if adapter
        .features()
        .contains(wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES)
    {
        required_features |= wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES;
    }

    let limits = adapter.limits();

    let (device, queue) = open_device(
        &adapter,
        "Processing Device",
        required_features,
        &limits,
        wgpu::MemoryHints::Performance,
        &GPU_DEVICE_LOST,
    )
    .inspect_err(|_| crate::render::gpu::fallback::abandon_attempt())?;

    #[cfg(not(target_os = "android"))]
    let display_opt = if let Some((surface, platform)) = surface_opt {
        let window = app_handle
            .get_webview_window("main")
            .ok_or("Failed to get main window")?;
        let size = window
            .inner_size()
            .unwrap_or(tauri::PhysicalSize::new(1280, 720));
        Some(build_wgpu_display(
            surface, &adapter, &device, &queue, size, false, platform,
        )?)
    } else {
        None
    };

    #[cfg(target_os = "android")]
    let display_opt = None;

    let new_context = GpuContext {
        instance,
        adapter,
        device,
        queue,
        limits,
        backend: adapter_info.backend,
        display: Arc::new(std::sync::Mutex::new(display_opt)),
        viewer_display: Arc::new(std::sync::Mutex::new(None)),
        pipelines: Arc::new(std::sync::OnceLock::new()),
        compute: Arc::new(std::sync::OnceLock::new()),
    };
    // BUG-2026-11-28-33 (v200/Mate 80): the background decode-warming thread
    // creates a SECOND wgpu device on the same adapter (decode_gpu_for →
    // request_device → Device::new → create_bind_group_layout), which the
    // Maleoon v200 driver crashes on (SIGSEGV @0x4e0, NULL pointer in
    // vkCreateDescriptorSetLayout). Serialized locks don't help — the crash
    // is the SECOND device creation itself, not a race. On OHOS, don't
    // pre-warm the decode GPU; it initializes lazily on the first decode
    // (same for compute pipelines). The cost is a slightly slower first
    // image open, which is already the warmup-crash-flag fallback.
    #[cfg(not(target_env = "ohos"))]
    if !new_context.shares_one_gl_context() {
        let warm = new_context.clone();
        std::thread::spawn(move || {
            // BUG-2026-11-28-25/26: this warming thread creates compute
            // pipelines (bind-group layouts -> vkCreateDescriptorSetLayout)
            // with NO lock, racing the pipeline-warmup thread and the main
            // path's processor creation. The Maleoon driver's internal
            // descriptor-layout container is not thread-safe: the racing
            // writes tear a pointer field (fault x1 is packed
            // non-pointer data / label bytes, constant per driver
            // generation) and the next vkCreate* dereferences it ->
            // SIGSEGV. Serialize ALL GPU object-creation bursts through
            // GPU_OBJECT_CREATION.
            let _guard = GPU_OBJECT_CREATION
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            crate::decode::gpu_fallback::prepare(&decode_gpu_for(&warm));
            warm.compute
                .get_or_init(|| crate::render::gpu::compute::ComputePipelines::new(&warm.device))
                .warm(&warm.device);
        });
    }
    *context_lock = Some(new_context.clone());
    Ok(new_context)
}

#[derive(Clone)]
pub struct GpuContext {
    #[cfg_attr(target_env = "ohos", allow(dead_code))] // keep-alive; OHOS is compute-only
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub limits: wgpu::Limits,
    pub backend: wgpu::Backend,
    pub display: Arc<std::sync::Mutex<Option<WgpuDisplay>>>,
    pub viewer_display: Arc<std::sync::Mutex<Option<WgpuDisplay>>>,
    pub pipelines: Arc<OnceLock<Arc<GpuPipelines>>>,
    pub compute: Arc<OnceLock<crate::render::gpu::compute::ComputePipelines>>,
}

impl GpuContext {
    pub fn shares_one_gl_context(&self) -> bool {
        self.backend == wgpu::Backend::Gl
    }

    pub fn display_for_label(
        &self,
        label: &str,
    ) -> Option<&Arc<std::sync::Mutex<Option<WgpuDisplay>>>> {
        match label {
            "main" => Some(&self.display),
            crate::platform::viewer_window::VIEWER_LABEL => Some(&self.viewer_display),
            _ => None,
        }
    }
}

pub fn lock_or_reset<'a, T>(
    mutex: &'a std::sync::Mutex<Option<T>>,
    name: &str,
) -> std::sync::MutexGuard<'a, Option<T>> {
    mutex.lock().unwrap_or_else(|poisoned| {
        log::warn!("{} lock was poisoned. Resetting to self-heal.", name);
        let mut guard = poisoned.into_inner();
        *guard = None;
        guard
    })
}
