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
    pub fn probe_node(
        relative: &str,
        fd: &OwnedFd,
    ) -> Result<Vec<gphoto2::camera::CameraDescriptor>, String> {
        wrap_sys_device(fd)?;
        let context = gphoto2::Context::new()
            .map_err(|e| format!("Failed to create gphoto2 context: {e}"))?;
        let cameras = gphoto2::Camera::autodetect(&context).map_err(|e| e.to_string())?;
        let _ = relative;
        Ok(cameras)
    }
}

#[cfg(all(feature = "tethering", target_os = "linux", target_env = "ohos"))]
fn list_cameras_blocking() -> Result<Vec<String>, String> {
    let nodes = ohos_usb::scan_nodes();
    if nodes.is_empty() {
        log::info!("[tether-ohos] no usbfs device nodes under /dev/bus/usb");
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for node in &nodes {
        match ohos_usb::open_node(node) {
            Ok(fd) => {
                log::info!("[tether-ohos] usbfs node {node} opened, probing");
                match ohos_usb::probe_node(node, &fd) {
                    Ok(cameras) => {
                        for camera in cameras {
                            out.push(format!("{} (usbfs:{})", camera.model, node));
                        }
                    }
                    Err(e) => log::warn!("[tether-ohos] probe on {node} failed: {e}"),
                }
                // OwnedFd drop closes the node.
            }
            Err(e) => {
                log::warn!("[tether-ohos] cannot open {node} (needs USB authorization?): {e}")
            }
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
                let mut found: Option<(gphoto2::camera::CameraDescriptor, std::os::fd::OwnedFd)> =
                    None;
                let nodes = ohos_usb::scan_nodes();
                if nodes.is_empty() {
                    return Err("No USB devices found under /dev/bus/usb - connect the camera with an OTG cable and set its USB mode to PC/Webcam/PTP.".into());
                }
                for node in nodes {
                    match ohos_usb::open_node(&node) {
                        Ok(fd) => match ohos_usb::probe_node(&node, &fd) {
                            Ok(cameras) => {
                                if let Some(camera) = cameras.into_iter().next() {
                                    log::info!(
                                        "[tether-ohos] connecting to {} ({}) via usbfs:{node}",
                                        camera.model,
                                        camera.port
                                    );
                                    found = Some((camera, fd));
                                    break;
                                }
                            }
                            Err(e) => log::warn!("[tether-ohos] probe {node}: {e}"),
                        },
                        Err(e) => {
                            return Err(format!("Cannot open USB device {node} ({e}). Grant USB access to the camera in system settings, then retry."))
                        }
                    }
                }
                let (camera, fd) =
                    found.ok_or("No camera found on the connected USB devices")?;
                ohos_usb::wrap_sys_device(&fd)
                    .map_err(|e| format!("Failed to hand USB device to libgphoto2: {e}"))?;
                {
                    let mut session = state.camera_session.lock().unwrap();
                    session.usb_fd = Some(fd);
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
                    &["meteringmode", "metering-mode", "exposuremeteringmode"],
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
                "meteringmode" => &["meteringmode", "metering-mode", "exposuremeteringmode"],
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
