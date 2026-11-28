use std::fs;
use std::io::Write;
use std::panic;

pub fn setup_logging(app_handle: &tauri::AppHandle) {
    let log_dir = match crate::app_paths::app_log_dir(app_handle) {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("Failed to get app log directory: {}", e);
            return;
        }
    };

    if let Err(e) = fs::create_dir_all(&log_dir) {
        eprintln!("Failed to create log directory at {:?}: {}", log_dir, e);
    }

    let log_file_path = log_dir.join("app.log");

    // Retain the previous session's log as app.log.1 before truncating:
    // QA evidence chains (import/export/AI runs) used to evaporate on every
    // restart because the file was opened with truncate(true) and no
    // rotation (BUG-2026-11-28-02). One generation is enough for the
    // cross-restart evidence the test rounds depend on. rename() is the
    // primary path; the el2/base sandbox supports it, but if it ever fails
    // (the tauri fork's FUSE-resolved variants denied rename with EACCES -
    // persist_downloaded_asset precedent) fall back to a byte copy.
    let rotated_path = log_dir.join("app.log.1");
    if log_file_path.exists() {
        let _ = fs::remove_file(&rotated_path);
        if fs::rename(&log_file_path, &rotated_path).is_err() {
            let _ = fs::copy(&log_file_path, &rotated_path);
        }
    }

    let log_file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&log_file_path)
        .ok();

    let var = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    let level: log::LevelFilter = var.parse().unwrap_or(log::LevelFilter::Info);

    let mut dispatch = fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{} [{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                message
            ))
        })
        // Coarse gate has to admit Debug so the libgphoto2 bridge below can be
        // forwarded while everything else stays at the configured level.
        .level(log::LevelFilter::Debug)
        .filter(move |metadata| {
            // libgphoto2 reports the useful diagnostics ("Could not load ...",
            // "No iolibs found in ...", libusb errors) at GP_LOG_DEBUG, which the
            // gphoto2 `log` feature forwards as log::Debug on target "gphoto2".
            // Those are exactly what we need when the tethering stack misbehaves,
            // so keep them regardless of RUST_LOG.
            if metadata.target() == "gphoto2" {
                return true;
            }
            !crate::decode::raw_processing::swallow_prefix_log(metadata)
                && metadata.level() <= level
        })
        .chain(fern::Output::call(|record| {
            let _ = writeln!(std::io::stderr(), "{}", record.args());
        }));

    if let Some(file) = log_file {
        dispatch = dispatch.chain(file);
    } else {
        eprintln!(
            "Failed to open log file at {:?}. Logging to console only.",
            log_file_path
        );
    }

    if let Err(e) = dispatch.apply() {
        eprintln!("Failed to apply logger configuration: {}", e);
    }

    panic::set_hook(Box::new(|info| {
        if crate::decode::raw_processing::is_reading_prefix() {
            return;
        }
        let message = if let Some(s) = info.payload().downcast_ref::<&'static str>() {
            s.to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            format!("{:?}", info.payload())
        };
        let location = info.location().map_or_else(
            || "at an unknown location".to_string(),
            |loc| format!("at {}:{}:{}", loc.file(), loc.line(), loc.column()),
        );
        log::error!("PANIC! {} - {}", location, message.trim());
    }));

    log::info!(
        "Logger initialized successfully. Log file at: {:?}",
        log_file_path
    );
}

#[tauri::command]
pub fn get_log_file_path(app_handle: tauri::AppHandle) -> Result<String, String> {
    let log_dir = crate::app_paths::app_log_dir(&app_handle).map_err(|e| e.to_string())?;
    let log_file_path = log_dir.join("app.log");
    Ok(log_file_path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn frontend_log(level: String, message: String) -> Result<(), String> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Ok(());
    }

    let log_line = |line: &str| match level.to_lowercase().as_str() {
        "error" => log::error!("[frontend] {}", line),
        "warn" => log::warn!("[frontend] {}", line),
        "debug" => log::debug!("[frontend] {}", line),
        "trace" => log::trace!("[frontend] {}", line),
        _ => log::info!("[frontend] {}", line),
    };

    for line in trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        log_line(line);
    }

    Ok(())
}
