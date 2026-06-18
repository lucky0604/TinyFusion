#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;
use tauri_plugin_shell::ShellExt;

#[tauri::command]
fn get_config() -> Result<serde_json::Value, String> {
    let path = dirs_next::home_dir()
        .ok_or("No home dir")?
        .join(".tinyfusion")
        .join("config.json");
    if !path.exists() {
        return Ok(serde_json::json!({
            "port": 9999,
            "workers": [],
            "judge": { "name": "judge", "endpoint": "http://localhost:11434", "model_id": "llama3", "api_key": null },
            "executor": { "name": "executor", "endpoint": "http://localhost:11434", "model_id": "llama3", "api_key": null },
            "workspaces": {},
            "error_keywords": []
        }));
    }
    let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&content).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_config(config: serde_json::Value) -> Result<(), String> {
    let path = dirs_next::home_dir()
        .ok_or("No home dir")?
        .join(".tinyfusion")
        .join("config.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, &json).map_err(|e| e.to_string())
}

#[tauri::command]
fn restart_core(app: tauri::AppHandle) -> Result<(), String> {
    stop_core_inner(&app);
    start_core_inner(&app)
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn stop_core_inner(app: &tauri::AppHandle) {
    use std::sync::Mutex;
    if let Some(state) = app.try_state::<Mutex<Option<tauri_plugin_shell::process::CommandChild>>>() {
        if let Ok(mut guard) = state.lock() {
            if let Some(child) = guard.take() {
                let pid = child.pid();
                eprintln!("[gui] stopping sidecar pid={}", pid);
                let _ = child.kill();
                // Poll with kill(pid,0) to confirm death. PID reuse within the
                // 5 s window is theoretically possible but vanishingly unlikely on
                // modern systems with PID randomization. If Tauri's CommandChild
                // gains a wait() API, prefer that over raw PID polling.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
                loop {
                    #[cfg(unix)]
                    {
                        unsafe { if libc::kill(pid as i32, 0) != 0 { break; } }
                    }
                    #[cfg(not(unix))]
                    break;
                    if std::time::Instant::now() >= deadline {
                        eprintln!("[gui] sidecar pid={} did not die, sending SIGKILL", pid);
                        #[cfg(unix)]
                        unsafe { libc::kill(pid as i32, libc::SIGKILL); }
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                eprintln!("[gui] sidecar pid={} stopped", pid);
            }
        }
    }
}

fn start_core_inner(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let sidecar = app.shell().sidecar("tinyfusion-core")
        .map_err(|e| format!("Sidecar not found: {}", e))?;
    let (mut rx, child) = sidecar.spawn()
        .map_err(|e| format!("Failed to spawn sidecar: {}", e))?;

    use std::sync::Mutex;
    app.manage(Mutex::new(Some(child)));

    tauri::async_runtime::spawn(async move {
        use tauri_plugin_shell::process::CommandEvent;
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    eprintln!("[core stdout] {}", String::from_utf8_lossy(&line));
                }
                CommandEvent::Stderr(line) => {
                    eprintln!("[core stderr] {}", String::from_utf8_lossy(&line));
                }
                CommandEvent::Terminated(status) => {
                    eprintln!("[core] exited with {:?}", status);
                    break;
                }
                _ => {}
            }
        }
    });

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let _ = start_core_inner(&app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            restart_core,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
