use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager};

/// Resolve light/dark dock icons from the bundle (or the icons/ folder in dev).
fn icon_path(app: &AppHandle, dark: bool) -> Result<PathBuf, String> {
    let file = if dark {
        "icon-dark.png"
    } else {
        "icon-light.png"
    };

    // Packaged app: resources declared in tauri.conf.json
    if let Ok(dir) = app.path().resource_dir() {
        let packaged = dir.join("icons").join(file);
        if packaged.exists() {
            return Ok(packaged);
        }
        let flat = dir.join(file);
        if flat.exists() {
            return Ok(flat);
        }
    }

    // Dev: src-tauri/icons next to the crate
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("icons")
        .join(file);
    if dev.exists() {
        return Ok(dev);
    }

    Err(format!("icon not found: {file}"))
}

#[cfg(target_os = "macos")]
fn set_dock_icon(path: &Path) -> Result<(), String> {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSString;

    let mtm = MainThreadMarker::new().ok_or("dock icon must be set on the main thread")?;
    let path_str = path.to_str().ok_or("icon path is not valid UTF-8")?;
    let ns_path = NSString::from_str(path_str);
    let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &ns_path)
        .ok_or_else(|| format!("could not load icon at {path_str}"))?;
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: standard AppKit dock-icon update; image was just loaded successfully.
    unsafe {
        app.setApplicationIconImage(Some(&image));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn set_dock_icon(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Switch the Dock / taskbar icon to match the OS appearance.
#[tauri::command]
pub fn set_app_icon_theme(app: AppHandle, dark: bool) -> Result<(), String> {
    let path = icon_path(&app, dark)?;
    app.run_on_main_thread(move || {
        if let Err(e) = set_dock_icon(&path) {
            eprintln!("set_app_icon_theme: {e}");
        }
    })
    .map_err(|e| e.to_string())
}
