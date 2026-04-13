//! Bambu Monitor — Single binary: HTTP server + MQTT + Tauri tray
//!
//! No Python required. Everything runs in one Rust binary.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager,
};
use tokio::sync::broadcast;

mod server;
use server::{start_http_server, mqtt_loop, SharedState};

// ── Tray icon ────────────────────────────────────────────────────────────────

fn make_tray_icon(color_rgb: (u8, u8, u8), progress: f64) -> tauri::image::Image<'static> {
    let size: u32 = 32;
    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let outer_r = size as f32 * 0.46;
    let inner_r = size as f32 * 0.34;
    let (cr, cg, cb) = color_rgb;
    let (bg_r, bg_g, bg_b) = (22u8, 25u8, 32u8);

    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    let start_angle = -std::f32::consts::FRAC_PI_2;
    let sweep = (progress.clamp(0.0, 1.0) as f32) * 2.0 * std::f32::consts::PI;
    let end_angle = start_angle + sweep;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let dist = (dx * dx + dy * dy).sqrt();

            if dist > outer_r + 1.5 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }

            let bg_alpha = if dist < outer_r - 1.0 {
                255u8
            } else {
                ((outer_r + 1.5 - dist).clamp(0.0, 1.0) * 255.0) as u8
            };

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

            let (r, g, b, a) = if in_ring {
                let cov = if dist >= inner_r && dist <= outer_r {
                    1.0f32
                } else if dist < inner_r {
                    ((dist - (inner_r - 1.5)) / 1.5).clamp(0.0, 1.0)
                } else {
                    ((outer_r + 1.5 - dist) / 1.5).clamp(0.0, 1.0)
                };
                (
                    (cr as f32 * cov + bg_r as f32 * (1.0 - cov)) as u8,
                    (cg as f32 * cov + bg_g as f32 * (1.0 - cov)) as u8,
                    (cb as f32 * cov + bg_b as f32 * (1.0 - cov)) as u8,
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

fn update_tray(app: &AppHandle, state: &str, progress: f64) {
    let (rgb, tooltip) = state_icon(state, progress);
    let icon = make_tray_icon(rgb, progress / 100.0);
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_icon(Some(icon));
        let _ = tray.set_tooltip(Some(&tooltip));
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

// ── OS notification ───────────────────────────────────────────────────────────

#[tauri::command]
async fn send_notification(app: AppHandle, title: String, body: String) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .builder()
        .title(&title)
        .body(&body)
        .show()
        .map_err(|e| e.to_string())
}

// ── Main ─────────────────────────────────────────────────────────────────────

static PRINT_WAS_RUNNING: AtomicBool = AtomicBool::new(false);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![send_notification])
        .setup(|app| {
            setup_tray(app.handle())?;

            // Shared state for HTTP + MQTT
            let (tx, _rx) = broadcast::channel::<server::PrinterState>(64);
            let shared = Arc::new(SharedState::new(tx));

            let app_handle = app.handle().clone();

            // HTTP server (tiny_http, runs in dedicated thread)
            let http_shared = shared.clone();
            start_http_server(http_shared);

            // MQTT client (rumqttc, async task)
            let mqtt_shared = shared.clone();
            tauri::async_runtime::spawn(async move {
                mqtt_loop(mqtt_shared).await;
            });

            // Tray icon updater — polls /api/status every 6s
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap();

            let app2 = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                let mut last_state = String::new();

                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(6)).await;

                    match client.get("http://localhost:5001/api/status").send().await {
                        Ok(resp) => {
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

                                if state != last_state || (progress as i32) % 30 == 0 {
                                    last_state = state.clone();
                                    update_tray(&app2, &state, progress);
                                }

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
                                    let _ = app2.emit("print-finished", serde_json::json!({ "job": job }));
                                }
                                if state.to_uppercase() == "RUNNING" {
                                    PRINT_WAS_RUNNING.store(true, Ordering::SeqCst);
                                }
                            }
                        }
                        Err(_) => {}
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
