use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::{sleep, Duration};

static BACKEND_RUNNING: AtomicBool = AtomicBool::new(false);
static PRINT_WAS_RUNNING: AtomicBool = AtomicBool::new(false);

// Python backend path — relative to the app bundle or project root
// In dev: resolves to <project>/backend/server.py
// In production: set BAMBU_BACKEND env or rely on bundled path
fn get_backend_path() -> String {
    std::env::var("BAMBU_BACKEND").unwrap_or_else(|_| {
        // Try relative to current working directory
        let cwd = std::env::current_dir().unwrap_or_default();
        let path = cwd.join("backend").join("server.py");
        path.to_string_lossy().to_string()
    })
}

fn get_venv_python() -> String {
    std::env::var("BAMBU_PYTHON").unwrap_or_else(|_| {
        let cwd = std::env::current_dir().unwrap_or_default();
        let path = cwd.join("backend").join("venv").join("bin").join("python3");
        if path.exists() {
            path.to_string_lossy().to_string()
        } else {
            "python3".to_string()
        }
    })
}

/// Generate a 22x22 macOS tray icon PNG: colored circle on transparent bg
fn make_tray_icon(color_rgb: (u8, u8, u8)) -> tauri::image::Image<'static> {
    let size = 44u32; // 2x for retina
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    let center = (size / 2) as f32;
    let radius = (size as f32) * 0.42;
    let (cr, cg, cb) = color_rgb;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let alpha = if dist <= radius - 1.0 {
                255u8
            } else if dist <= radius + 1.0 {
                let t = (radius + 1.0 - dist) / 2.0;
                (t * 255.0) as u8
            } else {
                0u8
            };
            rgba.extend_from_slice(&[cr, cg, cb, alpha]);
        }
    }

    tauri::image::Image::new_owned(rgba, size, size)
}

/// Map printer state → (rgb color, tooltip text)
fn state_icon(state: &str, progress: f64) -> ((u8, u8, u8), String) {
    let s = state.to_uppercase();
    match s.as_str() {
        "RUNNING" | "PRINTING" => {
            // Amber/yellow — printing
            let pct = progress as i32;
            ((0xFF, 0xB0, 0x00), format!("🖨 打印中 {}%", pct))
        }
        "PAUSE" | "PAUSED" => {
            // Orange — paused
            ((0xFF, 0x8C, 0x00), "⏸ 已暂停".into())
        }
        "FINISH" | "COMPLETED" => {
            // Green — done
            ((0x00, 0xD4, 0xAA), "✅ 已完成".into())
        }
        "IDLE" => {
            // Dim blue — idle
            ((0x64, 0x7E, 0x8C), "⏸ 待机".into())
        }
        "FAIL" | "ERROR" => {
            // Red
            ((0xFF, 0x45, 0x45), "❌ 错误".into())
        }
        _ => {
            // Gray — unknown
            ((0x88, 0x88, 0x88), "Bambu Monitor".into())
        }
    }
}

fn update_tray(app: &AppHandle, state: &str, progress: f64) {
    let (rgb, tooltip) = state_icon(state, progress);
    let icon = make_tray_icon(rgb);

    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_icon(Some(icon));
        let _ = tray.set_tooltip(Some(&tooltip));
    }
}

#[tauri::command]
pub async fn start_backend(app: AppHandle) -> Result<(), String> {
    if !BACKEND_RUNNING.swap(true, Ordering::SeqCst) {
        log::info!("Starting Python backend…");
        spawn_python(app).await;
    }
    Ok(())
}

async fn spawn_python(app: AppHandle) {
    let python = get_venv_python();
    let server = get_backend_path();

    let mut child = Command::new(&python)
        .arg(&server)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("Failed to start Python backend");

    // Forward Python stdout lines to log
    if let Some(stdout) = child.stdout.take() {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        log::info!("[py] {}", line);
                        if line.contains("MQTT connected") {
                            let _ = app2.emit("backend-connected", ());
                        }
                    }
                    Ok(None) => break,
                    Err(e) => { log::warn!("[py stdout] {}", e); break; }
                }
            }
        });
    }

    // Poll /api/status every 6 s to watch print state transitions
    let app3 = app.clone();
    tauri::async_runtime::spawn(async move {
        sleep(Duration::from_secs(3)).await;
        poll_status(app3).await;
    });

    // Clean up when Python exits
    let app4 = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = child.wait().await;
        BACKEND_RUNNING.store(false, Ordering::SeqCst);
        let _ = app4.emit("backend-disconnected", ());
    });
}

/// Poll http://localhost:5001/api/status, fire Tauri events on state changes.
async fn poll_status(app: AppHandle) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => { log::error!("reqwest client error: {}", e); return; }
    };

    let mut last_state = String::new();

    while BACKEND_RUNNING.load(Ordering::SeqCst) {
        match client.get("http://localhost:5001/api/status").send().await {
            Ok(resp) => {
                match resp.json::<serde_json::Value>().await {
                    Ok(json) => {
                        let state = json.get("gcode_state")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        let job = json.get("job_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("—")
                            .to_string();

                        let progress = json.get("progress")
                            .and_then(|v| v.as_f64())
                            .unwrap_or(0.0);

                        // Update tray icon on state change or progress tick
                        if state != last_state {
                            log::info!("State: {} → {}", last_state, state);
                            last_state = state.clone();
                        }
                        update_tray(&app, &state, progress);

                        let _ = app.emit("printer-state", serde_json::json!({
                            "state": state,
                            "job": job,
                            "progress": progress
                        }));

                        let was_running = PRINT_WAS_RUNNING.load(Ordering::SeqCst);
                        let is_finish = state == "FINISH"
                            || state == "COMPLETED"
                            || state == "finish"
                            || state == "completed";

                        if was_running && is_finish {
                            PRINT_WAS_RUNNING.store(false, Ordering::SeqCst);
                            let _ = app.emit("print-finished", serde_json::json!({ "job": job }));
                        }

                        if state == "RUNNING" || state == "printing" {
                            PRINT_WAS_RUNNING.store(true, Ordering::SeqCst);
                        }
                    }
                    Err(e) => { log::warn!("status parse error: {}", e); }
                }
            }
            Err(e) => { log::debug!("status poll error: {}", e); }
        }
        sleep(Duration::from_secs(6)).await;
    }
}

#[tauri::command]
pub async fn stop_backend() -> Result<(), String> {
    BACKEND_RUNNING.store(false, Ordering::SeqCst);
    Ok(())
}
