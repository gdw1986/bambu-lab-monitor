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
// In production (Tauri bundle): looks next to the executable, then in Resources/
fn get_backend_path() -> String {
    std::env::var("BAMBU_BACKEND").unwrap_or_else(|_| {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                // Try: <exe_dir>/backend/server.py
                let rel = parent.join("backend").join("server.py");
                if rel.exists() {
                    return rel.to_string_lossy().to_string();
                }
                // Try: <exe_dir>/../Resources/backend/server.py  (macOS .app bundle)
                if let Some(grand) = parent.parent() {
                    let res = grand.join("Resources").join("backend").join("server.py");
                    if res.exists() {
                        return res.to_string_lossy().to_string();
                    }
                }
            }
        }
        // Dev fallback: relative to CWD
        let cwd = std::env::current_dir().unwrap_or_default();
        cwd.join("backend").join("server.py").to_string_lossy().to_string()
    })
}

fn get_venv_python() -> String {
    std::env::var("BAMBU_PYTHON").unwrap_or_else(|_| {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                let venv = parent.join("backend").join("venv").join("bin").join("python3");
                if venv.exists() {
                    return venv.to_string_lossy().to_string();
                }
            }
        }
        let cwd = std::env::current_dir().unwrap_or_default();
        let venv = cwd.join("backend").join("venv").join("bin").join("python3");
        if venv.exists() {
            venv.to_string_lossy().to_string()
        } else {
            "python3".to_string()
        }
    })
}

/// Render a 32×32 tray icon: dark background circle + colored progress arc (antialiased).
/// progress: 0.0 – 1.0  (0 = no arc, idle state)
fn make_tray_icon(color_rgb: (u8, u8, u8), progress: f64) -> tauri::image::Image<'static> {
    let size: u32 = 32;
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let outer_r = size as f32 * 0.46;
    let inner_r = size as f32 * 0.34;

    let (cr, cg, cb) = color_rgb;
    let bg_r: u8 = 22;
    let bg_g: u8 = 25;
    let bg_b: u8 = 32;

    let mut rgba = Vec::with_capacity((size * size * 4) as usize);

    let start_angle = -std::f32::consts::FRAC_PI_2; // 12 o'clock
    let sweep = (progress.clamp(0.0, 1.0) as f32) * 2.0 * std::f32::consts::PI;
    let end_angle = start_angle + sweep;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();

            // Outside circle → transparent
            if dist > outer_r + 1.5 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }

            // Background alpha (anti-aliased edge)
            let bg_alpha = if dist < outer_r - 1.0 {
                255u8
            } else {
                ((outer_r + 1.5 - dist).clamp(0.0, 1.0) * 255.0) as u8
            };

            // Progress arc check
            let in_ring = if sweep > 0.005 && dist >= inner_r - 1.5 && dist <= outer_r + 1.5 {
                let angle = dy.atan2(dx);
                let a = if angle < -std::f32::consts::FRAC_PI_2 {
                    angle + 2.0 * std::f32::consts::PI
                } else {
                    angle
                };
                a >= start_angle - 0.05 && a <= end_angle + 0.05
            } else {
                false
            };

            let (r, g, b, a) = if in_ring {
                let arc_cov = if dist >= inner_r && dist <= outer_r {
                    1.0f32
                } else if dist < inner_r {
                    ((dist - (inner_r - 1.5)) / 1.5).clamp(0.0, 1.0)
                } else {
                    ((outer_r + 1.5 - dist) / 1.5).clamp(0.0, 1.0)
                };
                let t = arc_cov;
                (
                    (cr as f32 * t + bg_r as f32 * (1.0 - t)) as u8,
                    (cg as f32 * t + bg_g as f32 * (1.0 - t)) as u8,
                    (cb as f32 * t + bg_b as f32 * (1.0 - t)) as u8,
                    255u8,
                )
            } else {
                (bg_r, bg_g, bg_b, bg_alpha)
            };

            rgba.extend_from_slice(&[r, g, b, a]);
        }
    }

    tauri::image::Image::new_owned(rgba, size, size)
}

/// Map printer state → (rgb color, tooltip text)
fn state_icon_info(state: &str, progress: f64) -> ((u8, u8, u8), String) {
    let s = state.to_uppercase();
    match s.as_str() {
        "RUNNING" | "PRINTING" => {
            let pct = progress as i32;
            ((0xFF, 0xB0, 0x00), format!("🖨 打印中 {}%", pct))
        }
        "PAUSE" | "PAUSED" => {
            ((0xFF, 0x8C, 0x00), "⏸ 已暂停".into())
        }
        "FINISH" | "COMPLETED" | "SUCCESS" => {
            ((0x00, 0xD4, 0xAA), "✅ 已完成".into())
        }
        "IDLE" => {
            ((0x64, 0x7E, 0x8C), "⏸ 待机".into())
        }
        "FAIL" | "ERROR" => {
            ((0xFF, 0x45, 0x45), "❌ 错误".into())
        }
        _ => {
            ((0x88, 0x88, 0x88), "Bambu Monitor".into())
        }
    }
}

fn update_tray(app: &AppHandle, state: &str, progress: f64) {
    let (rgb, tooltip) = state_icon_info(state, progress);
    let icon = make_tray_icon(rgb, progress / 100.0); // progress is 0-100 from printer

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

    let app3 = app.clone();
    tauri::async_runtime::spawn(async move {
        sleep(Duration::from_secs(3)).await;
        poll_status(app3).await;
    });

    let app4 = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = child.wait().await;
        BACKEND_RUNNING.store(false, Ordering::SeqCst);
        let _ = app4.emit("backend-disconnected", ());
    });
}

/// Poll http://localhost:5001/api/status, update tray icon and emit events on state changes.
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

                        if state != last_state {
                            log::info!("State: {} → {}", last_state, state);
                            last_state = state.clone();
                        }
                        // Always update tray icon (progress changes every poll)
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
                            || state == "completed"
                            || state == "SUCCESS";

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
