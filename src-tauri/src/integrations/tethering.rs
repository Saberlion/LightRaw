#![allow(unused_variables)]

use serde::Serialize;
use std::collections::HashMap;
use tauri::ipc::Response;

#[cfg(feature = "tethering")]
use crate::app::state::AppState;
#[cfg(feature = "tethering")]
use tauri::Manager;

#[derive(Serialize)]
pub struct CameraConfigChoice {
    pub name: String,
    pub current_value: String,
    pub choices: Vec<String>,
}

#[cfg(feature = "tethering")]
pub struct CameraSession {
    pub context: Option<gphoto2::Context>,
    pub camera: Option<gphoto2::Camera>,
    /// usbfs fd wrapped into libgphoto2 via gp_port_usb_set_sys_device on
    /// OHOS; dropped on the next connect, which closes the node.
    #[cfg(all(target_os = "linux", target_env = "ohos"))]
    pub usb_fd: Option<std::os::fd::OwnedFd>,
    /// usbManager `USBDevice.name` (e.g. "1-3") this session is connected
    /// through on OHOS. A hotplug detach matches the session against the
    /// device that left the bus, so unrelated devices cannot tear it down.
    #[cfg(all(target_os = "linux", target_env = "ohos"))]
    pub usb_device_name: Option<String>,
}

#[cfg(not(feature = "tethering"))]
pub struct CameraSession {}

impl CameraSession {
    pub fn new() -> Self {
        #[cfg(feature = "tethering")]
        {
            Self {
                context: gphoto2::Context::new().ok(),
                camera: None,
                #[cfg(all(target_os = "linux", target_env = "ohos"))]
                usb_fd: None,
                #[cfg(all(target_os = "linux", target_env = "ohos"))]
                usb_device_name: None,
            }
        }
        #[cfg(not(feature = "tethering"))]
        {
            Self {}
        }
    }
}

/// OpenHarmony USB open layer (docs/OHOS_USB_TETHERING_PLAN.md §3.1).
///
/// OHOS does not give apps global USB enumeration; the only route to a USB
/// camera is opening its authorized usbfs node and handing the fd to
/// libgphoto2 via `gp_port_usb_set_sys_device()` (the upstream Android-style
/// "authorized fd" path; camlibs/usb driver then wrap it with
/// libusb_wrap_sys_device + LIBUSB_OPTION_NO_DEVICE_DISCOVERY). Everything
/// after the wrap — autodetect, open, capture, config — is the stock
/// libgphoto2 flow shared with the desktop.
#[cfg(all(feature = "tethering", target_os = "linux", target_env = "ohos"))]
mod ohos_usb {
    use std::fs::{self, OpenOptions};
    use std::os::fd::{AsRawFd, OwnedFd};

    const USBFS_ROOT: &str = "/dev/bus/usb";

    /// Install libgphoto2's own log forwarding exactly once.
    ///
    /// libgphoto2 reports the interesting failures at GP_LOG_DEBUG - including
    /// the ones that decide whether tethering can work at all:
    /// `Could not load '<iolib>': '<reason>'` and `No iolibs found in '<dir>'`
    /// (the usb1 driver deciding it cannot serve), plus libusb's own messages.
    /// Without this bridge `Camera::autodetect` only surfaces a bare
    /// `GP_ERROR_LIBRARY (-4)` and the call that actually failed is invisible.
    fn ensure_gphoto_logging() {
        use std::sync::Mutex;

        static HANDLER: Mutex<Option<gphoto2::log_bridge::LogHandler>> = Mutex::new(None);

        // libusb reads LIBUSB_DEBUG itself (libusb_init_context -> getenv) and
        // writes its internal trace to stderr, i.e. hilog on OHOS. libgphoto2's
        // bridge above only covers libgphoto2's own messages, and the calls that
        // decide whether the wrapped fd works (libusb_init, wrap_sys_device,
        // claim_interface) are libusb's - so turn on its trace too.
        // SAFETY: called once, before any gphoto2/libusb call on this process.
        if std::env::var_os("LIBUSB_DEBUG").is_none() {
            unsafe { std::env::set_var("LIBUSB_DEBUG", "4") };
        }

        let mut guard = HANDLER.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return;
        }
        match gphoto2::log_bridge::forward_to_log(gphoto2::log_bridge::LogLevel::Debug) {
            Ok(handler) => {
                log::info!("[tether-ohos] libgphoto2 debug logging enabled");
                *guard = Some(handler);
            }
            Err(e) => log::warn!("[tether-ohos] could not enable libgphoto2 logging: {e}"),
        }
    }

    /// All usbfs device nodes as "BBB/DDD" (bus/dev, zero padded), sorted.
    pub fn scan_nodes() -> Vec<String> {
        let mut nodes = Vec::new();
        let Ok(buses) = fs::read_dir(USBFS_ROOT) else {
            return nodes;
        };
        for bus in buses.flatten() {
            let bus_name = bus.file_name().to_string_lossy().into_owned();
            let Ok(devs) = fs::read_dir(bus.path()) else {
                continue;
            };
            for dev in devs.flatten() {
                let dev_name = dev.file_name().to_string_lossy().into_owned();
                if dev_name.starts_with('.') {
                    continue;
                }
                nodes.push(format!("{bus_name}/{dev_name}"));
            }
        }
        nodes.sort();
        nodes
    }

    /// Open one usbfs node read/write. The fd must come from a device the
    /// user authorized (or one the platform already grants us); the io error
    /// string carries the errno for diagnosis.
    pub fn open_node(relative: &str) -> Result<OwnedFd, String> {
        let path = format!("{USBFS_ROOT}/{relative}");
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map(From::from)
            .map_err(|e| format!("{path}: {e}"))
    }

    /// Hand an already-opened, authorized fd to libgphoto2. From here the
    /// usb1 io driver only ever sees this one device.
    pub fn wrap_sys_device(fd: &OwnedFd) -> Result<(), String> {
        ensure_gphoto_logging();
        // SAFETY: gp_port_usb_set_sys_device stores the fd in a static; the
        // fd stays valid for as long as the caller keeps the OwnedFd alive.
        let rc = unsafe { gphoto2_sys::gp_port_usb_set_sys_device(fd.as_raw_fd()) };
        if rc < 0 {
            Err(format!("gp_port_usb_set_sys_device failed: {rc}"))
        } else {
            Ok(())
        }
    }

    /// Wrap + probe one node: returns the cameras autodetected on it (with
    /// the port names libgphoto2 synthesized for the wrapped device).
    ///
    /// This mirrors the known-good HarmonyOS libusb sequence
    /// (`libusb_set_option(NO_DEVICE_DISCOVERY)` -> `libusb_init` ->
    /// `libusb_wrap_sys_device(fd)`); libgphoto2's usb1 driver performs it
    /// internally once `gp_port_usb_set_sys_device` published the fd, so the
    /// steps below just record which stage the stack got to.
    pub fn probe_node(
        relative: &str,
        fd: &OwnedFd,
    ) -> Result<Vec<gphoto2::camera::CameraDescriptor>, String> {
        wrap_sys_device(fd).inspect_err(|e| log::warn!("[tether-ohos] {relative}: {e}"))?;
        log::info!(
            "[tether-ohos] {relative}: fd {} handed to gp_port_usb_set_sys_device",
            fd.as_raw_fd()
        );
        let context = gphoto2::Context::new()
            .map_err(|e| format!("Failed to create gphoto2 context: {e}"))?;
        match gphoto2::Camera::autodetect(&context) {
            Ok(cameras) => {
                log::info!(
                    "[tether-ohos] {relative}: autodetect -> {} camera(s)",
                    cameras.len()
                );
                Ok(cameras)
            }
            Err(e) => {
                log::warn!(
                    "[tether-ohos] {relative}: autodetect failed: {e} (see gphoto2:* lines above for the failing io driver / libusb call)"
                );
                Err(e.to_string())
            }
        }
    }
}

/// Bridge to the ArkTS side for `@ohos.usbManager` (docs/OHOS_USB_TETHERING_PLAN.md).
///
/// HarmonyOS labels `/dev/bus/usb` as SELinux `dev_bus_usb_file` and denies app
/// domains `read`/`getattr` on it, so scanning usbfs yields nothing however the
/// camera is attached (measured on MOR-M1 with a Sony ILCE-6700: the node is
/// there, the app sees an empty directory). The sanctioned route is
/// `getDevices` -> `requestRight` (system dialog) -> `connectDevice` ->
/// `getFileDescriptor`; that fd is what libgphoto2's usb1 driver consumes.
///
/// ArkTS polls [`ohos_tether_usb_wanted`]; the Rust side raises the flag when it
/// needs the grant, waits briefly, and receives the result through
/// [`notify_ohos_tether_usb_device`] / [`notify_ohos_tether_usb_error`].
#[cfg(all(feature = "tethering", target_os = "linux", target_env = "ohos"))]
mod usb_manager {
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, MutexGuard};
    use std::time::{Duration, Instant};

    use napi_ohos::bindgen_prelude::Result as NapiResult;

    pub struct GrantedDevice {
        pub name: String,
        pub vendor_id: u32,
        pub product_id: u32,
        pub fd: OwnedFd,
    }

    static GRANTED: Mutex<Vec<GrantedDevice>> = Mutex::new(Vec::new());
    static WANTED: AtomicBool = AtomicBool::new(false);
    static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

    unsafe extern "C" {
        fn dup(fd: i32) -> i32;
    }

    fn granted_guard() -> MutexGuard<'static, Vec<GrantedDevice>> {
        GRANTED.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Independent dups so probing never steals the session's fds.
    pub fn snapshot() -> Vec<GrantedDevice> {
        granted_guard()
            .iter()
            .filter_map(|device| {
                device.fd.try_clone().ok().map(|fd| GrantedDevice {
                    name: device.name.clone(),
                    vendor_id: device.vendor_id,
                    product_id: device.product_id,
                    fd,
                })
            })
            .collect()
    }

    /// Drop a grant whose fd no longer works (camera unplugged or dropped into
    /// power-save: the usbfs fd stays open after a dup but every ioctl on it
    /// fails with ENODEV, so libusb_wrap_sys_device returns IO and the probe
    /// finds nothing). Without this the dead entry is retried by every panel
    /// poll and connect attempt - the retry storm that exhausted libusb's
    /// pthread keys and aborted the process (SIGABRT, MOR-M1 2026-11-22
    /// 17:22). After forget() the next connect finds GRANTED empty, raises the
    /// want flag and gets a fresh grant (new device name -> new system dialog,
    /// or a silent re-connect when the right is still valid).
    pub fn forget(name: &str) {
        let mut devices = granted_guard();
        let before = devices.len();
        devices.retain(|d| d.name != name);
        if devices.len() != before {
            log::info!("[tether-ohos] dropped dead grant {name}");
        }
    }

    pub fn raise_want() {
        // Clear any stale verdict (e.g. the silent startup probe reporting
        // "no camera") so the interactive wait below is not aborted by it.
        *LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = None;
        WANTED.store(true, Ordering::SeqCst);
    }

    pub fn take_error() -> Option<String> {
        LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    fn has_error() -> bool {
        LAST_ERROR
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// Wait up to `timeout` for ArkTS to complete the permission flow. Bails
    /// out as soon as ArkTS reports a definitive answer (no camera attached,
    /// user denied) so the UI is not stuck behind the full timeout.
    pub fn wait_for_grant(timeout: Duration) -> Vec<GrantedDevice> {
        let current = snapshot();
        if !current.is_empty() {
            return current;
        }
        raise_want();
        log::info!("[tether-ohos] asking ArkTS for USB manager access");
        let deadline = Instant::now() + timeout;
        loop {
            std::thread::sleep(Duration::from_millis(120));
            let devices = snapshot();
            if !devices.is_empty() {
                log::info!(
                    "[tether-ohos] usbManager granted {} device(s)",
                    devices.len()
                );
                return devices;
            }
            if has_error() || Instant::now() >= deadline {
                return Vec::new();
            }
        }
    }

    /// NAPI export (ArkTS: `ohosTetherUsbWanted`). True once per request so the
    /// poller does not repeat the permission dialog.
    #[napi_derive_ohos::napi]
    pub fn ohos_tether_usb_wanted() -> bool {
        WANTED.swap(false, Ordering::SeqCst)
    }

    /// NAPI export (ArkTS: `notifyOhosTetherUsbDevice`). Called after
    /// `requestRight` + `connectDevice` + `getFileDescriptor`.
    #[napi_derive_ohos::napi]
    pub fn notify_ohos_tether_usb_device(
        name: String,
        vendor_id: u32,
        product_id: u32,
        fd: i32,
    ) -> NapiResult<()> {
        if fd < 0 {
            return Err(napi_ohos::Error::from_reason(format!(
                "invalid fd {fd} for {name}"
            )));
        }
        // SAFETY: `dup` on a live fd owned by the ArkTS pipe. The policy here
        // is to keep the duplicate and open the pipe exclusively from it, so
        // ArkTS closing the pipe never pulls the descriptor out from under us.
        let dup_fd = unsafe { dup(fd) };
        if dup_fd < 0 {
            return Err(napi_ohos::Error::from_reason(format!(
                "dup({fd}) failed for {name}"
            )));
        }
        // SAFETY: dup_fd is a fresh descriptor owned by us from here on.
        let owned = unsafe { OwnedFd::from_raw_fd(dup_fd) };
        log::info!(
            "[tether-ohos] usbManager device {name} vid={vendor_id:04x} pid={product_id:04x} fd={dup_fd}"
        );
        let mut devices = granted_guard();
        devices.retain(|d| d.name != name);
        devices.push(GrantedDevice {
            name: name.clone(),
            vendor_id,
            product_id,
            fd: owned,
        });
        drop(devices);

        // Probe once, right here: this is the first moment libgphoto2 can talk
        // to the device, and it is the difference between "USB plumbing works"
        // and "the camera actually answers". Logging it makes a bad fd or an
        // unsupported transfer mode show up in app.log without the user having
        // to open the tethering panel first.
        let mut model = None;
        if let Ok(dup_fd) = granted_guard()
            .iter()
            .find(|d| d.name == name)
            .and_then(|d| d.fd.try_clone().ok())
            .ok_or(())
        {
            match crate::integrations::tethering::ohos_usb::probe_node(&name, &dup_fd) {
                Ok(cameras) if !cameras.is_empty() => {
                    for camera in &cameras {
                        log::info!(
                            "[tether-ohos] fd probe OK: {} on {} ({})",
                            camera.model,
                            name,
                            camera.port
                        );
                    }
                    model = Some(cameras[0].model.clone());
                }
                Ok(_) => {
                    log::warn!("[tether-ohos] fd probe: {name} answered but reported no camera")
                }
                Err(e) => log::warn!("[tether-ohos] fd probe failed on {name}: {e}"),
            }
        }
        // Tell the webview a (potential) camera arrived, so the panel refreshes
        // its camera list right away instead of waiting for its next poll.
        crate::ohos_integration::emit_to_webview(
            "tether-usb-changed",
            serde_json::json!({ "event": "attached", "name": name, "model": model }),
        );
        Ok(())
    }

    /// NAPI export (ArkTS: `notifyOhosTetherUsbDetached`). EntryAbility forwards
    /// the system COMMON_EVENT_USB_DEVICE_DETACHED common event (reconciled
    /// against getDevices, since the event does not say which device left):
    /// this device dropped off the bus - unplugged, power-save
    /// re-enumeration, cable wiggle.
    ///
    /// Three resources must go, timely:
    /// 1. the grant entry (its fd is a corpse - probing it again is the retry
    ///    storm `forget` was written to stop);
    /// 2. the camera session when it belongs to this device (the webview gets
    ///    the `tether-usb-changed` event and disconnects immediately, instead
    ///    of three seconds later through the poll);
    /// 3. the ArkTS pipe, closed by the caller before this is invoked.
    #[napi_derive_ohos::napi]
    pub fn notify_ohos_tether_usb_detached(name: String) -> NapiResult<()> {
        log::info!("[tether-ohos] usbManager device detached: {name}");
        forget(&name);
        crate::ohos_integration::emit_to_webview(
            "tether-usb-changed",
            serde_json::json!({ "event": "detached", "name": name }),
        );
        // The session teardown runs off the ArkTS main thread: this NAPI
        // callback is on the UI thread, and camera_session is held for whole
        // operations (a capture can hold it for seconds) - blocking here
        // would freeze the app. Dropping the Camera may also talk to the
        // (dead) device once more; in spawn_blocking that costs a thread,
        // not the UI.
        tauri::async_runtime::spawn_blocking(move || {
            use tauri::Manager;
            crate::ohos_integration::with_app_handle(|handle| {
                let state = handle.state::<crate::app::state::AppState>();
                let mut session = state.camera_session.lock().unwrap();
                if session.usb_device_name.as_deref() == Some(name.as_str()) {
                    log::info!("[tether-ohos] dropping session of detached device {name}");
                    session.camera = None;
                    // Closes the dup; the ArkTS pipe behind the grant is
                    // closed by the caller.
                    session.usb_fd = None;
                    session.usb_device_name = None;
                }
            });
        });
        Ok(())
    }

    /// NAPI export (ArkTS: `notifyOhosTetherUsbError`).
    #[napi_derive_ohos::napi]
    pub fn notify_ohos_tether_usb_error(reason: String) -> NapiResult<()> {
        log::warn!("[tether-ohos] usbManager reported: {reason}");
        *LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = Some(reason);
        Ok(())
    }
}

#[cfg(all(feature = "tethering", target_os = "linux", target_env = "ohos"))]
const USB_GRANT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

#[cfg(all(feature = "tethering", target_os = "linux", target_env = "ohos"))]
fn list_cameras_blocking() -> Result<Vec<String>, String> {
    let mut out = Vec::new();

    // 1) Devices already authorized through the system dialog. Read-only: the
    //    panel polls this every few seconds, so it must neither block on the
    //    permission flow nor trigger the requestRight dialog - only an
    //    explicit tether_connect does that (via wait_for_grant).
    for device in usb_manager::snapshot() {
        match ohos_usb::probe_node(&device.name, &device.fd) {
            Ok(cameras) => {
                for camera in cameras {
                    out.push(format!("{} ({})", camera.model, device.name));
                }
            }
            Err(e) => {
                log::warn!("[tether-ohos] probe {} failed: {e}", device.name);
                // A grant that cannot be probed is dead (device gone behind
                // the fd). Drop it so the poll stops retrying it; the next
                // connect re-requests a fresh grant instead.
                usb_manager::forget(&device.name);
            }
        }
    }

    // 2) Legacy usbfs fallback for platforms that still expose the nodes.
    for node in ohos_usb::scan_nodes() {
        let Ok(fd) = ohos_usb::open_node(&node) else {
            continue;
        };
        match ohos_usb::probe_node(&node, &fd) {
            Ok(cameras) => {
                for camera in cameras {
                    out.push(format!("{} (usbfs:{})", camera.model, node));
                }
            }
            Err(e) => log::warn!("[tether-ohos] probe on {node} failed: {e}"),
        }
    }

    Ok(out)
}

#[cfg(all(
    feature = "tethering",
    not(all(target_os = "linux", target_env = "ohos"))
))]
fn list_cameras_blocking() -> Result<Vec<String>, String> {
    let context =
        gphoto2::Context::new().map_err(|e| format!("Failed to create gphoto2 context: {e}"))?;
    let cameras = gphoto2::Camera::autodetect(&context).map_err(|e| e.to_string())?;
    Ok(cameras
        .into_iter()
        .map(|c| format!("{} ({})", c.model, c.port))
        .collect())
}

unsafe impl Send for CameraSession {}
unsafe impl Sync for CameraSession {}

#[tauri::command]
pub async fn tether_list_cameras() -> Result<Vec<String>, String> {
    #[cfg(feature = "tethering")]
    {
        tauri::async_runtime::spawn_blocking(list_cameras_blocking)
            .await
            .map_err(|e| format!("Task panicked: {}", e))?
    }
    #[cfg(not(feature = "tethering"))]
    Err("Tethering is not supported in this build.".into())
}

#[tauri::command]
pub async fn tether_connect(app_handle: tauri::AppHandle) -> Result<String, String> {
    #[cfg(feature = "tethering")]
    {
        tauri::async_runtime::spawn_blocking(move || {
            let state = app_handle.state::<AppState>();

            {
                let mut session = state.camera_session.lock().unwrap();
                session.camera = None;
                // Drop the previous wrapped usbfs fd (OHOS) — closes the node.
                #[cfg(all(target_os = "linux", target_env = "ohos"))]
                {
                    session.usb_fd = None;
                    session.usb_device_name = None;
                }
            }

            std::thread::sleep(std::time::Duration::from_millis(150));

            let context = gphoto2::Context::new()
                .map_err(|e| format!("Failed to initialize gphoto2 context: {}", e))?;

            #[cfg(all(target_os = "linux", target_env = "ohos"))]
            let descriptor = {
                // OHOS: wrap the first usbfs node that opens and reports a
                // camera (docs/OHOS_USB_TETHERING_PLAN.md §3.1). The fd lives
                // in CameraSession for the whole session.
                let mut found: Option<(gphoto2::camera::CameraDescriptor, usb_manager::GrantedDevice)> =
                    None;
                let devices = usb_manager::wait_for_grant(USB_GRANT_TIMEOUT);
                if devices.is_empty() {
                    return Err(usb_manager::take_error().unwrap_or_else(|| {
                        "No camera authorized. Connect it with an OTG cable, allow the system USB access dialog, and set the camera's USB mode to PC Remote/PTP.".to_string()
                    }));
                }
                for device in devices {
                    match ohos_usb::probe_node(&device.name, &device.fd) {
                        Ok(cameras) => {
                            if let Some(camera) = cameras.into_iter().next() {
                                log::info!(
                                    "[tether-ohos] connecting to {} ({}) via usbManager",
                                    camera.model,
                                    device.name
                                );
                                found = Some((camera, device));
                                break;
                            }
                        }
                        Err(e) => {
                            log::warn!("[tether-ohos] probe {}: {e}", device.name);
                            // Dead fd (camera unplugged/power-save): drop the
                            // grant so the next attempt re-requests a fresh
                            // one instead of reusing this corpse.
                            usb_manager::forget(&device.name);
                        }
                    }
                }
                let (camera, device) = found.ok_or(
                    "The authorized USB device did not report as a camera - set its USB mode to PC Remote/PTP and retry.",
                )?;
                ohos_usb::wrap_sys_device(&device.fd)
                    .map_err(|e| format!("Failed to hand USB device to libgphoto2: {e}"))?;
                {
                    let mut session = state.camera_session.lock().unwrap();
                    // Keep the fd (and the ArkTS pipe behind it) alive for the
                    // whole session; dropping it here would close the device.
                    session.usb_fd = Some(device.fd);
                    session.usb_device_name = Some(device.name.clone());
                }
                camera
            };
            #[cfg(not(all(target_os = "linux", target_env = "ohos")))]
            let descriptor = {
                let cameras = gphoto2::Camera::autodetect(&context).map_err(|e| e.to_string())?;
                cameras.into_iter().next().ok_or("No camera found")?
            };

            let camera = match gphoto2::Camera::open(&context, &descriptor.model, &descriptor.port)
            {
                Ok(cam) => cam,
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    gphoto2::Camera::open(&context, &descriptor.model, &descriptor.port)
                        .map_err(|e| format!("Failed to connect to camera: {}", e))?
                }
            };

            if let Ok(widget) = camera.get_single_config(&context, "capturetarget") {
                let choices = widget.choices().unwrap_or_default();
                if let Some(ram_choice) = choices.iter().find(|c| {
                    let lower = c.to_lowercase();
                    lower.contains("ram") || lower.contains("sdram")
                }) {
                    let _ = widget.set_choice(ram_choice);
                    let _ = camera.set_single_config(&context, "capturetarget", &widget);
                }
            }

            for drive_key in &["drivemode", "drive-mode"] {
                if let Ok(widget) = camera.get_single_config(&context, drive_key) {
                    let choices = widget.choices().unwrap_or_default();
                    if let Some(single_choice) = choices.iter().find(|c| {
                        let lower = c.to_lowercase();
                        lower.contains("single") || lower == "1" || lower.contains("standard")
                    }) {
                        let _ = widget.set_choice(single_choice);
                        let _ = camera.set_single_config(&context, drive_key, &widget);
                        break;
                    }
                }
            }

            let _ = camera.capture_preview(&context);

            for _ in 0..15 {
                if let Ok(event) =
                    camera.wait_for_event(&context, std::time::Duration::from_millis(30))
                    && let gphoto2::CameraEvent::Timeout = event
                {
                    break;
                }
            }

            let model_name = descriptor.model.clone();

            {
                let mut session = state.camera_session.lock().unwrap();
                session.context = Some(context);
                session.camera = Some(camera);
            }

            Ok(format!("Connected to {}", model_name))
        })
        .await
        .map_err(|e| format!("Task panicked: {}", e))?
    }
    #[cfg(not(feature = "tethering"))]
    Err("Tethering is not supported in this build.".into())
}

#[tauri::command]
pub async fn tether_get_settings(
    app_handle: tauri::AppHandle,
) -> Result<HashMap<String, CameraConfigChoice>, String> {
    #[cfg(feature = "tethering")]
    {
        tauri::async_runtime::spawn_blocking(move || {
            use gphoto2::widget::WidgetValue;

            let state = app_handle.state::<AppState>();
            let session = state.camera_session.lock().unwrap();
            let camera = session.camera.as_ref().ok_or("No camera connected")?;
            let context = session.context.as_ref().ok_or("No context initialized")?;

            let keys_to_query: [(&str, &[&str]); 8] = [
                ("iso", &["iso", "iso-speed", "ISO"]),
                (
                    "shutterspeed",
                    &["shutterspeed", "exposure-time", "exposurespeed"],
                ),
                ("aperture", &["f-number", "aperture", "fnumber"]),
                ("whitebalance", &["whitebalance", "white-balance"]),
                (
                    "colortemperature",
                    &[
                        "colortemperature",
                        "colortemp",
                        "wbcolortemperature",
                        "whitebalancetemperature",
                    ],
                ),
                (
                    "exposurecompensation",
                    &["exposurecompensation", "exposurebias", "exposure-bias"],
                ),
                (
                    "exposuremode",
                    &[
                        "autoexposuremode",
                        "expprogram",
                        "shootingmode",
                        "exposure-program",
                    ],
                ),
                (
                    "meteringmode",
                    &[
                        "meteringmode",
                        "metering-mode",
                        "exposuremeteringmode",
                        // ptp2 registers PTP_DPC_ExposureMeteringMode under
                        // "exposuremetermode" (camlibs/ptp2/config.c menus
                        // table); the extra "ing" spelling never matches.
                        "exposuremetermode",
                    ],
                ),
            ];

            let mut attempts = 0;
            let mut map = HashMap::new();

            while attempts < 3 {
                map.clear();

                for &b_key in &["batterylevel", "battery", "battery-level"] {
                    if let Ok(widget) = camera.get_single_config(context, b_key) {
                        let current_value = match widget.value() {
                            Ok(WidgetValue::Choice(s)) => s,
                            Ok(WidgetValue::Text(s)) => s,
                            Ok(WidgetValue::Range(f)) => f.to_string(),
                            _ => continue,
                        };
                        map.insert(
                            "batterylevel".to_string(),
                            CameraConfigChoice {
                                name: "batterylevel".to_string(),
                                current_value,
                                choices: vec![],
                            },
                        );
                        break;
                    }
                }

                for (frontend_key, aliases) in keys_to_query.iter() {
                    for &camera_key in *aliases {
                        if let Ok(widget) = camera.get_single_config(context, camera_key) {
                            let current_value = match widget.value() {
                                Ok(WidgetValue::Choice(s)) => s,
                                Ok(WidgetValue::Text(s)) => s,
                                Ok(WidgetValue::Range(f)) => f.to_string(),
                                _ => continue,
                            };

                            let choices = widget.choices().unwrap_or_default();

                            map.insert(
                                frontend_key.to_string(),
                                CameraConfigChoice {
                                    name: frontend_key.to_string(),
                                    current_value,
                                    choices,
                                },
                            );
                            break;
                        }
                    }
                }

                let has_glitched_values = map.values().any(|cfg| {
                    cfg.current_value.contains("65535")
                        || cfg.current_value.contains("Unknown value 0000")
                });

                if !has_glitched_values && !map.is_empty() {
                    break;
                }

                attempts += 1;
                let _ = camera.wait_for_event(context, std::time::Duration::from_millis(200));
            }

            Ok(map)
        })
        .await
        .map_err(|e| format!("Task panicked: {}", e))?
    }
    #[cfg(not(feature = "tethering"))]
    Err("Tethering is not supported in this build.".into())
}

#[tauri::command]
pub async fn tether_set_setting(
    app_handle: tauri::AppHandle,
    setting_name: String,
    value: String,
) -> Result<(), String> {
    #[cfg(feature = "tethering")]
    {
        tauri::async_runtime::spawn_blocking(move || {
            use gphoto2::widget::WidgetType;

            let state = app_handle.state::<AppState>();
            let session = state.camera_session.lock().unwrap();
            let camera = session.camera.as_ref().ok_or("No camera connected")?;
            let context = session.context.as_ref().ok_or("No context initialized")?;

            let aliases: &[&str] = match setting_name.as_str() {
                "iso" => &["iso", "iso-speed", "ISO"],
                "shutterspeed" => &["shutterspeed", "exposure-time", "exposurespeed"],
                "aperture" => &["f-number", "aperture", "fnumber"],
                "whitebalance" => &["whitebalance", "white-balance"],
                "colortemperature" => &[
                    "colortemperature",
                    "colortemp",
                    "wbcolortemperature",
                    "whitebalancetemperature",
                ],
                "exposurecompensation" => {
                    &["exposurecompensation", "exposurebias", "exposure-bias"]
                }
                "exposuremode" => &[
                    "autoexposuremode",
                    "expprogram",
                    "shootingmode",
                    "exposure-program",
                ],
                "meteringmode" => &[
                    "meteringmode",
                    "metering-mode",
                    "exposuremeteringmode",
                    // matches the ptp2 widget name for PTP_DPC_ExposureMeteringMode
                    "exposuremetermode",
                ],
                _ => &[],
            };

            let mut applied = false;
            let mut last_err = None;

            for &camera_key in aliases {
                if let Ok(widget) = camera.get_single_config(context, camera_key)
                    && let Ok(widget_type) = widget.widget_type()
                {
                    let set_ok = match widget_type {
                        WidgetType::Radio | WidgetType::Menu => widget.set_choice(&value).is_ok(),
                        WidgetType::Text => widget.set_text(&value).is_ok(),
                        _ => {
                            let clean_val = value.replace("K", "").trim().to_string();
                            if let Ok(parsed_f) = clean_val.parse::<f32>() {
                                widget
                                    .set_value(&gphoto2::widget::WidgetValue::Range(parsed_f))
                                    .is_ok()
                            } else {
                                false
                            }
                        }
                    };

                    if set_ok {
                        match camera.set_single_config(context, camera_key, &widget) {
                            Ok(_) => {
                                applied = true;
                                break;
                            }
                            Err(e) => {
                                let msg = e.to_string().to_lowercase();
                                if msg.contains("read-only")
                                    || msg.contains("readonly")
                                    || msg.contains("locked")
                                {
                                    last_err = Some(format!(
                                        "{} is physically locked (e.g. physical dial).",
                                        setting_name
                                    ));
                                } else {
                                    last_err =
                                        Some(format!("Failed to set {}: {}", setting_name, e));
                                }
                            }
                        }
                    }
                }
            }

            if !applied {
                if let Some(err) = last_err {
                    return Err(err);
                }
                return Err(format!("Failed to set {} to {}", setting_name, value));
            }

            Ok(())
        })
        .await
        .map_err(|e| format!("Task panicked: {}", e))?
    }
    #[cfg(not(feature = "tethering"))]
    Err("Tethering is not supported in this build.".into())
}

#[tauri::command]
pub async fn tether_autofocus(app_handle: tauri::AppHandle) -> Result<(), String> {
    #[cfg(feature = "tethering")]
    {
        tauri::async_runtime::spawn_blocking(move || {
            let state = app_handle.state::<AppState>();
            let session = state.camera_session.lock().unwrap();
            let camera = session.camera.as_ref().ok_or("No camera connected")?;
            let context = session.context.as_ref().ok_or("No context initialized")?;

            let mut triggered_key = None;

            for &af_key in &["autofocusdrive", "autofocus", "eosviewfinder"] {
                if let Ok(widget) = camera.get_single_config(context, af_key) {
                    let mut success = false;

                    if widget.set_choice("1").is_ok() || widget.set_choice("On").is_ok() {
                        if camera.set_single_config(context, af_key, &widget).is_ok() {
                            success = true;
                        }
                    } else if widget
                        .set_value(&gphoto2::widget::WidgetValue::Toggle(true))
                        .is_ok()
                        && camera.set_single_config(context, af_key, &widget).is_ok()
                    {
                        success = true;
                    }

                    if success {
                        triggered_key = Some(af_key);
                        break;
                    }
                }
            }

            let _ = camera.wait_for_event(context, std::time::Duration::from_millis(1000));

            if let Some(key) = triggered_key
                && let Ok(widget) = camera.get_single_config(context, key)
            {
                let _ = widget.set_choice("0");
                let _ = widget.set_choice("Off");
                let _ = widget.set_value(&gphoto2::widget::WidgetValue::Toggle(false));
                let _ = camera.set_single_config(context, key, &widget);
            }

            Ok(())
        })
        .await
        .map_err(|e| format!("Task panicked: {}", e))?
    }
    #[cfg(not(feature = "tethering"))]
    Err("Tethering is not supported in this build.".into())
}

#[tauri::command]
pub async fn tether_get_preview(app_handle: tauri::AppHandle) -> Result<Response, String> {
    #[cfg(feature = "tethering")]
    {
        tauri::async_runtime::spawn_blocking(move || {
            let state = app_handle.state::<AppState>();
            let session = state.camera_session.lock().unwrap();
            let camera = session.camera.as_ref().ok_or("No camera connected")?;
            let context = session.context.as_ref().ok_or("No context initialized")?;

            let file = camera
                .capture_preview(context)
                .map_err(|e| format!("Failed to capture preview: {}", e))?;

            let data = file.data().map_err(|e| e.to_string())?;

            Ok(Response::new(data.to_vec()))
        })
        .await
        .map_err(|e| format!("Task panicked: {}", e))?
    }
    #[cfg(not(feature = "tethering"))]
    Err("Tethering is not supported in this build.".into())
}

#[tauri::command]
pub async fn tether_capture(
    app_handle: tauri::AppHandle,
    destination_folder: Option<String>,
) -> Result<String, String> {
    #[cfg(feature = "tethering")]
    {
        tauri::async_runtime::spawn_blocking(move || {
            let state = app_handle.state::<AppState>();
            let session = state.camera_session.lock().unwrap();
            let camera = session.camera.as_ref().ok_or("No camera connected")?;
            let context = session.context.as_ref().ok_or("No context initialized")?;

            while let Ok(event) =
                camera.wait_for_event(context, std::time::Duration::from_millis(10))
            {
                if let gphoto2::CameraEvent::Timeout = event {
                    break;
                }
            }

            let camera_file_path = camera
                .capture_image(context)
                .map_err(|e| format!("Capture failed: {}", e))?;

            let file = camera
                .download(context, &camera_file_path)
                .map_err(|e| format!("Download failed: {}", e))?;

            let save_dir = destination_folder
                .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().to_string());
            let output_path = std::path::Path::new(&save_dir).join(&camera_file_path.name);

            std::fs::write(&output_path, file.data().map_err(|e| e.to_string())?)
                .map_err(|e| format!("Failed to save captured file: {}", e))?;

            let _ = camera.delete_file(context, &camera_file_path.folder, &camera_file_path.name);

            Ok(output_path.to_string_lossy().to_string())
        })
        .await
        .map_err(|e| format!("Task panicked: {}", e))?
    }
    #[cfg(not(feature = "tethering"))]
    Err("Tethering is not supported in this build.".into())
}
