use tauri::Window;

#[tauri::command]
pub fn desktop_window_minimize(window: Window) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn desktop_window_toggle_maximize(window: Window) -> Result<(), String> {
    let is_max = window.is_maximized().map_err(|e| e.to_string())?;
    if is_max {
        window.unmaximize().map_err(|e| e.to_string())
    } else {
        window.maximize().map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub fn desktop_window_close(window: Window) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}
