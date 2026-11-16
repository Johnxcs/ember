use tauri::Manager;

// Remember the server address between launches (stored in the app's data dir).
#[tauri::command]
fn load_cfg(app: tauri::AppHandle) -> String {
    app.path()
        .app_data_dir()
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join("server.txt")).ok())
        .unwrap_or_default()
}

#[tauri::command]
fn save_cfg(app: tauri::AppHandle, url: String) -> Result<(), String> {
    let d = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    std::fs::write(d.join("server.txt"), url).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![load_cfg, save_cfg])
        .run(tauri::generate_context!())
        .expect("error while running Ember");
}
