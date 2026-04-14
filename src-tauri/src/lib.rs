//! Bambu Monitor — Single binary: HTTP server + MQTT + Tauri tray
//!
//! No Python required. Everything runs in one Rust binary.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use log::{error, info, warn};
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tokio::sync::broadcast;

mod server;
mod storage;
use server::{start_http_server, mqtt_loop, SharedState};

// ── Tray icon ────────────────────────────────────────────────────────────────

/// 五档进度: 0, 25, 50, 75, 100
fn progress_level(progress: f64) -> i32 {
    let p = progress.clamp(0.0, 100.0);
    if p >= 100.0 { 100 }
    else if p >= 75.0 { 75 }
    else if p >= 50.0 { 50 }
    else if p >= 25.0 { 25 }
    else { 0 }
}

/// 绘制数字到像素缓冲区 (5x3 像素字体)
fn draw_digit(buf: &mut Vec<u8>, x: usize, y: usize, w: usize, digit: u8, color: (u8, u8, u8)) {
    let patterns: [[u8; 5]; 10] = [
        [0b111, 0b101, 0b101, 0b101, 0b111], // 0
        [0b010, 0b110, 0b010, 0b010, 0b111], // 1
        [0b111, 0b001, 0b111, 0b100, 0b111], // 2
        [0b111, 0b001, 0b111, 0b001, 0b111], // 3
        [0b101, 0b101, 0b111, 0b001, 0b001], // 4
        [0b111, 0b100, 0b111, 0b001, 0b111], // 5
        [0b111, 0b100, 0b111, 0b101, 0b111], // 6
        [0b111, 0b001, 0b001, 0b001, 0b001], // 7
        [0b111, 0b101, 0b111, 0b101, 0b111], // 8
        [0b111, 0b101, 0b111, 0b001, 0b111], // 9
    ];
    
    let pattern = patterns[digit as usize];
    for row in 0..5 {
        for col in 0..3 {
            if pattern[row] & (1 << (2 - col)) != 0 {
                let idx = ((y + row) * w + (x + col)) * 4;
                if idx + 2 < buf.len() {
                    buf[idx] = color.0;
                    buf[idx + 1] = color.1;
                    buf[idx + 2] = color.2;
                    buf[idx + 3] = 255;
                }
            }
        }
    }
}

/// 绘制进度数字 (0-100) 到图标中心
fn draw_progress_number(buf: &mut Vec<u8>, w: usize, h: usize, progress: i32, color: (u8, u8, u8)) {
    let prog = progress.clamp(0, 100);
    let digits = if prog >= 100 {
        vec![1, 0, 0]
    } else if prog >= 10 {
        vec![(prog / 10) as u8, (prog % 10) as u8]
    } else {
        vec![prog as u8]
    };
    
    let digit_w = 3;
    let digit_h = 5;
    let spacing = 1;
    let total_w = digits.len() * digit_w + (digits.len() - 1) * spacing;
    let start_x = (w - total_w) / 2;
    let start_y = (h - digit_h) / 2;
    
    for (i, &digit) in digits.iter().enumerate() {
        draw_digit(buf, start_x + i * (digit_w + spacing), start_y, w, digit, color);
    }
}

fn make_tray_icon(color_rgb: (u8, u8, u8), progress: f64) -> tauri::image::Image<'static> {
    let size: u32 = 32;
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let outer_r = size as f32 * 0.46;
    let inner_r = size as f32 * 0.34;
    let (cr, cg, cb) = color_rgb;
    let (bg_r, bg_g, bg_b) = (22u8, 25u8, 32u8);

    let mut rgba = vec![bg_r, bg_g, bg_b, 0]; // Start with transparent
    rgba.resize((size * size * 4) as usize, 0);
    
    // 五档进度: 0%, 25%, 50%, 75%, 100%
    let level = progress_level(progress * 100.0);
    let sweep = (level as f32 / 100.0) * 2.0 * std::f32::consts::PI;
    let start_angle = -std::f32::consts::FRAC_PI_2;
    let end_angle = start_angle + sweep;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = ((y * size + x) * 4) as usize;

            if dist > outer_r + 1.5 {
                rgba[idx + 3] = 0; // Transparent
                continue;
            }

            let bg_alpha = if dist < outer_r - 1.0 {
                255u8
            } else {
                ((outer_r + 1.5 - dist).clamp(0.0, 1.0) * 255.0) as u8
            };

            // 绘制进度环
            let in_ring = sweep > 0.005
                && dist >= inner_r - 1.5
                && dist <= outer_r + 1.5
                && {
                    let angle = dy.atan2(dx);
                    let a = if angle < -std::f32::consts::FRAC_PI_2 {
                        angle + 2.0 * std::f32::consts::PI
                    } else {
                        angle
                    };
                    a >= start_angle - 0.05 && a <= end_angle + 0.05
                };

            if in_ring {
                let cov = if dist >= inner_r && dist <= outer_r {
                    1.0f32
                } else if dist < inner_r {
                    ((dist - (inner_r - 1.5)) / 1.5).clamp(0.0, 1.0)
                } else {
                    ((outer_r + 1.5 - dist) / 1.5).clamp(0.0, 1.0)
                };
                rgba[idx] = (cr as f32 * cov + bg_r as f32 * (1.0 - cov)) as u8;
                rgba[idx + 1] = (cg as f32 * cov + bg_g as f32 * (1.0 - cov)) as u8;
                rgba[idx + 2] = (cb as f32 * cov + bg_b as f32 * (1.0 - cov)) as u8;
                rgba[idx + 3] = 255;
            } else if dist < inner_r - 2.0 {
                // 内部背景 - 稍暗
                rgba[idx] = bg_r;
                rgba[idx + 1] = bg_g;
                rgba[idx + 2] = bg_b;
                rgba[idx + 3] = bg_alpha;
            } else {
                rgba[idx] = bg_r;
                rgba[idx + 1] = bg_g;
                rgba[idx + 2] = bg_b;
                rgba[idx + 3] = bg_alpha;
            }
        }
    }

    // 在中心绘制进度数字
    draw_progress_number(&mut rgba, size as usize, size as usize, level, (255, 255, 255));

    tauri::image::Image::new_owned(rgba, size, size)
}

fn state_icon(state: &str, progress: f64) -> ((u8, u8, u8), String) {
    match state.to_uppercase().as_str() {
        "RUNNING" | "PRINTING" => ((0xFF, 0xB0, 0x00), format!("🖨 打印中 {}%", progress as i32)),
        "PAUSE" | "PAUSED" => ((0xFF, 0x8C, 0x00), "⏸ 已暂停".into()),
        "FINISH" | "COMPLETED" | "SUCCESS" => ((0x00, 0xD4, 0xAA), "✅ 已完成".into()),
        "IDLE" | "PREPARE" => ((0x64, 0x7E, 0x8C), "⏸ 待机".into()),
        "FAIL" | "ERROR" => ((0xFF, 0x45, 0x45), "❌ 错误".into()),
        _ => ((0x88, 0x88, 0x88), "Bambu Monitor".into()),
    }
}

fn update_tray(app: &AppHandle, shared: &Arc<SharedState>, state: &str, progress: f64) {
    let (rgb, tooltip) = state_icon(state, progress);
    let icon = make_tray_icon(rgb, progress / 100.0);
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_icon(Some(icon));
        let _ = tray.set_tooltip(Some(&tooltip));
    }
    
    // Control floating window visibility based on printing state
    let is_printing = matches!(
        state.to_uppercase().as_str(),
        "RUNNING" | "PRINTING" | "PAUSE" | "PAUSED"
    );
    
    if let Some(window) = app.get_webview_window("floating-progress") {
        // Get additional data from shared state for floating window
        let state_guard = shared.state.read().unwrap();
        let bed_temp = state_guard.bed_temp as i32;
        let nozzle_temp = state_guard.nozzle_temp as i32;
        let remaining_time = state_guard.remaining_time;
        let job_name = state_guard.job_name.clone();
        drop(state_guard);
        
        // Always emit state update to floating window when it's visible or should be visible
        eprintln!("[Tray] Emitting state to floating window: state={}, progress={}", state, progress);
        let result = window.emit("printer-state", serde_json::json!({
            "state": state,
            "progress": progress,
            "bed_temp": bed_temp,
            "nozzle_temp": nozzle_temp,
            "remaining_time": remaining_time,
            "job_name": job_name
        }));
        if let Err(e) = result {
            eprintln!("[Tray] Failed to emit to floating window: {}", e);
        }

        if is_printing && progress >= 0.0 && progress < 100.0 {
            let _ = window.show();
        } else if state.to_uppercase() == "FINISH" || state.to_uppercase() == "COMPLETED" {
            // Keep visible for a moment on completion, then hide
            let app_clone = app.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                if let Some(w) = app_clone.get_webview_window("floating-progress") {
                    let _ = w.hide();
                }
            });
        }
    }
}

// ── Tray setup ───────────────────────────────────────────────────────────────

fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let show = MenuItemBuilder::with_id("show", "显示窗口").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "退出").build(app)?;

    let menu = MenuBuilder::new(app).item(&show).separator().item(&quit).build()?;

    let _tray = TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("Bambu Monitor")
        .on_menu_event(|app: &AppHandle, event: tauri::menu::MenuEvent| {
            match event.id().as_ref() {
                "show" => {
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray: &tauri::tray::TrayIcon, event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(win) = tray.app_handle().get_webview_window("main") {
                    if win.is_visible().unwrap_or(false) {
                        let _ = win.hide();
                    } else {
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }
            }
        })
        .build(app)?;

    Ok(())
}

// ── OS notification removed ───────────────────────────────────────────────────
// Notifications are now shown only in the tray icon tooltip, not as OS popups

#[tauri::command]
fn get_http_port() -> u16 {
    server::http_port()
}

#[tauri::command]
fn get_debug_info() -> String {
    format!(
        "http_port={} mqtt_connected={} app_version={} tauri_version={}",
        server::http_port(),
        server::mqtt_connected(),
        env!("CARGO_PKG_VERSION"),
        tauri::VERSION,
    )
}

#[tauri::command]
async fn save_config(host: String, serial: String, access_code: String) -> Result<bool, String> {
    log::info!("[save_config] Called with host={} serial={} access_code={}", host, serial, access_code);
    
    // Persist the config
    let persisted = storage::PersistedConfig {
        host: host.clone(),
        serial: serial.clone(),
        access_code: access_code.clone(),
    };
    
    if let Err(e) = storage::save_persisted_config(&persisted) {
        log::error!("[save_config] Failed to save config: {}", e);
        return Err(format!("Failed to save: {}", e));
    }
    
    log::info!("[save_config] Config saved to file, calling notify_config_update");
    
    // Notify the server to reload config
    server::notify_config_update();
    
    log::info!("[save_config] notify_config_update returned");
    
    Ok(true)
}

// ── Main ─────────────────────────────────────────────────────────────────────

static PRINT_WAS_RUNNING: AtomicBool = AtomicBool::new(false);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![get_http_port, get_debug_info, save_config])
        .setup(|app| {
            eprintln!("[SETUP] app data dir = {:?}", app.path().app_data_dir());
            info!("=== Bambu Monitor setup starting ===");

            setup_tray(app.handle()).map_err(|e| {
                error!("setup_tray FAILED: {}", e);
                e
            })?;
            info!("Tray icon OK");

            // Shared state for HTTP + MQTT
            let (tx, _rx) = broadcast::channel::<server::PrinterState>(64);
            let shared = Arc::new(SharedState::new(tx));
            
            // Set global shared state for config updates
            *server::GLOBAL_SHARED.lock().unwrap() = Some(shared.clone());

            let app_handle = app.handle().clone();

            // HTTP server (tiny_http, runs in dedicated thread)
            let http_shared = shared.clone();
            info!("Calling start_http_server()...");
            let _jh = start_http_server(http_shared);
            let port = server::http_port();
            info!("start_http_server() returned, http_port() = {}", port);

            if port == 0 {
                warn!("HTTP server returned port 0 — possible binding failure");
            } else {
                info!("HTTP server ready on port {}", port);
            }

            // MQTT client (rumqttc, async task)
            let mqtt_shared = shared.clone();
            eprintln!("[SETUP] Spawning mqtt_loop task...");
            tauri::async_runtime::spawn(async move {
                mqtt_loop(mqtt_shared).await;
            });
            eprintln!("[SETUP] mqtt_loop task spawned");

            info!("=== Bambu Monitor setup complete ===");

            // Create floating progress window (always on top, small, shows progress ring)
            let monitor = app.primary_monitor().ok().flatten();
            let (win_x, win_y) = if let Some(m) = monitor {
                let size = m.size();
                // Position at bottom-right with 20px margin
                (size.width as f64 - 140.0, size.height as f64 - 140.0)
            } else {
                (1600.0, 900.0)  // Fallback
            };
            
            eprintln!("[SETUP] Creating floating window at ({}, {})", win_x, win_y);
            let _float_window = tauri::WebviewWindowBuilder::new(
                app,
                "floating-progress",
                tauri::WebviewUrl::App("/floating.html".into())
            )
            .title("")
            .inner_size(140.0, 140.0)
            .position(win_x, win_y)
            .always_on_top(true)
            .decorations(false)       // No window decorations
            .transparent(true)        // Allow transparent background
            .skip_taskbar(true)       // Don't show in taskbar
            .resizable(false)
            .visible(false)           // Hidden by default, shown when printing
            .shadow(false)            // No window shadow (removes the square border)
            .build()
            .ok();
            
            if _float_window.is_some() {
                eprintln!("[SETUP] Floating window created successfully");
            } else {
                eprintln!("[SETUP] Failed to create floating window");
            }

            // Tray icon updater — reads from shared state directly (updated by MQTT)
            let tray_shared = shared.clone();
            let app2 = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let mut last_state = String::new();
                let mut last_progress: i32 = -1;

                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                    // Read directly from shared state (updated by MQTT in real-time)
                    let state = tray_shared.state.read().unwrap();
                    let state_str = state.gcode_state.clone();
                    let progress = state.progress;
                    drop(state);

                    // Update tray if state or progress changed significantly
                    let progress_int = progress as i32;
                    if state_str != last_state || (progress_int - last_progress).abs() >= 1 {
                        last_state = state_str.clone();
                        last_progress = progress_int;
                        update_tray(&app2, &tray_shared, &state_str, progress);
                    }
                }
            });

            // HTTP poller for frontend events (notifications removed - now shown in tray only)
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap();

            let app2 = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let mut consecutive_errors: u32 = 0;

                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(6)).await;

                    let port = server::http_port();
                    if port == 0 { 
                        if consecutive_errors % 5 == 4 {
                            warn!("HTTP server port is 0, backend may not be ready (consecutive: {})", consecutive_errors + 1);
                        }
                        consecutive_errors += 1;
                        continue; 
                    }
                    
                    let url = format!("http://localhost:{}/api/status", port);
                    match client.get(&url).send().await {
                        Ok(resp) => {
                            if consecutive_errors > 0 {
                                info!("Backend connection restored after {} failed attempts", consecutive_errors);
                                consecutive_errors = 0;
                            }

                            if !resp.status().is_success() {
                                warn!("Status API returned status {}", resp.status());
                                consecutive_errors += 1;
                                continue;
                            }

                            if let Ok(json) = resp.json::<serde_json::Value>().await {
                                let state = json
                                    .get("gcode_state")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let progress = json
                                    .get("progress")
                                    .and_then(|v| v.as_f64())
                                    .unwrap_or(0.0);
                                let job = json
                                    .get("job_name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("—")
                                    .to_string();

                                // Emit event for frontend (tray shows progress visually)
                                let _ = app2.emit("printer-state", serde_json::json!({
                                    "state": state,
                                    "job": job,
                                    "progress": progress
                                }));

                                let was_running = PRINT_WAS_RUNNING.load(Ordering::SeqCst);
                                let is_finish = matches!(
                                    state.to_uppercase().as_str(),
                                    "FINISH" | "COMPLETED" | "SUCCESS"
                                );
                                if was_running && is_finish {
                                    PRINT_WAS_RUNNING.store(false, Ordering::SeqCst);
                                    // Send desktop notification for print completion
                                    use tauri::Emitter;
                                    let _ = app2.emit("print-complete", serde_json::json!({
                                        "title": "打印完成",
                                        "body": format!("任务 '{}' 已完成", job)
                                    }));
                                }
                                if state.to_uppercase() == "RUNNING" || state.to_uppercase() == "PRINTING" {
                                    PRINT_WAS_RUNNING.store(true, Ordering::SeqCst);
                                }
                            } else {
                                warn!("Failed to parse JSON response from status API");
                                consecutive_errors += 1;
                            }
                        }
                        Err(e) => {
                            consecutive_errors += 1;
                            // Only log error periodically to avoid log spam
                            if consecutive_errors % 10 == 0 || consecutive_errors <= 2 {
                                warn!("Failed to fetch printer state (attempt {}): {}", consecutive_errors, e);
                            }
                            
                            // If too many consecutive errors, try to trigger a health check
                            if consecutive_errors >= 20 && server::mqtt_connected() {
                                warn!("Too many errors, checking MQTT connection status");
                            }
                        }
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
