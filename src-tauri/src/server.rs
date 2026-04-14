//! Bambu Monitor — Embedded HTTP server + MQTT client (no Python needed).
//!
//! Endpoints:
//!   GET  /api/status    — latest printer state as JSON
//!   GET  /api/config    — current host / serial
//!   POST /api/config    — update host / serial / access_code
//!   GET  /events        — SSE stream of real-time state
//!   GET  /              — serve index.html

use std::collections::HashMap;
use std::io::Read;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::Arc;
use std::sync::RwLock;
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Module-level HTTP server port, written once at bind time, read by Tauri commands.
static HTTP_PORT: AtomicU16 = AtomicU16::new(0);
/// Module-level MQTT connected flag, written by mqtt_loop, read by get_debug_info.
static MQTT_CONNECTED: AtomicBool = AtomicBool::new(false);
/// Flag to signal MQTT loop to reconnect with updated config
static MQTT_RECONNECT_NEEDED: AtomicBool = AtomicBool::new(true);  // Start with true so initial connection works

use chrono::Local;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use rumqttc::{AsyncClient, Event as MqttEvent, MqttOptions, Packet, QoS, Transport};
use rumqttc::TlsConfiguration;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use rustls::client::danger::{ServerCertVerifier, ServerCertVerified, HandshakeSignatureValid};
use rustls::pki_types::UnixTime;
use std::fmt::Debug;

// ── Types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrinterState {
    #[serde(rename = "gcode_state")]
    pub gcode_state: String,
    pub mode: String,
    pub action: String,
    pub progress: f64,
    #[serde(rename = "remaining_time")]
    pub remaining_time: u64,
    #[serde(rename = "nozzle_temp")]
    pub nozzle_temp: f64,
    #[serde(rename = "nozzle_target")]
    pub nozzle_target: f64,
    #[serde(rename = "bed_temp")]
    pub bed_temp: f64,
    #[serde(rename = "bed_target")]
    pub bed_target: f64,
    #[serde(rename = "layer_current")]
    pub layer_current: u32,
    #[serde(rename = "layer_total")]
    pub layer_total: u32,
    pub speed: String,
    #[serde(rename = "filament_type")]
    pub filament_type: String,
    pub ams: HashMap<String, AmsSlot>,
    #[serde(rename = "job_name")]
    pub job_name: String,
    #[serde(rename = "live_speed")]
    pub live_speed: u32,
    pub light: String,
    pub online: bool,
    #[serde(rename = "last_update")]
    pub last_update: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmsSlot {
    pub color: String,
    pub material: String,
    pub remaining: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterConfig {
    pub host: String,
    pub serial: String,
    pub access_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigUpdate {
    pub host: Option<String>,
    pub serial: Option<String>,
    #[serde(rename = "access_code")]
    pub access_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigResponse {
    pub host: String,
    pub serial: String,
    #[serde(rename = "has_access_code")]
    pub has_access_code: bool,
}

// ── Shared state ─────────────────────────────────────────────────────────────

pub struct SharedState {
    pub state: RwLock<PrinterState>,
    pub config: RwLock<PrinterConfig>,
    pub tx: broadcast::Sender<PrinterState>,
}

impl SharedState {
    pub fn new(tx: broadcast::Sender<PrinterState>) -> Self {
        // Load persisted config
        let persisted = crate::storage::load_persisted_config();
        
        Self {
            state: RwLock::new(PrinterState {
                mode: "unknown".into(),
                action: "unknown".into(),
                gcode_state: "UNKNOWN".into(),
                progress: 0.0,
                remaining_time: 0,
                nozzle_temp: 0.0,
                nozzle_target: 0.0,
                bed_temp: 0.0,
                bed_target: 0.0,
                layer_current: 0,
                layer_total: 0,
                speed: "100".into(),
                filament_type: "".into(),
                ams: HashMap::new(),
                job_name: "".into(),
                live_speed: 0,
                light: "off".into(),
                online: false,
                last_update: "".into(),
            }),
            config: RwLock::new(PrinterConfig {
                host: persisted.host,
                serial: persisted.serial,
                access_code: persisted.access_code,
            }),
            tx,
        }
    }
}

// ── MQTT payload parser ───────────────────────────────────────────────────────

fn parse_hex_color(hex: &str) -> String {
    if hex.len() >= 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
        format!("#{:02X}{:02X}{:02X}", r, g, b)
    } else {
        hex.to_string()
    }
}

fn process_mqtt_payload(obj: &serde_json::Value, state: &mut PrinterState) {
    let print_data = match obj.get("print").and_then(|v| v.as_object()) {
        Some(m) => m.clone(),
        None => return,
    };

    let gcode_state = print_data
        .get("gcode_state")
        .and_then(|v| v.as_str())
        .unwrap_or("UNKNOWN");
    let is_full = !gcode_state.is_empty() && gcode_state != "UNKNOWN";

    let remaining_time = print_data
        .get("mc_remaining_time")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let progress = print_data
        .get("mc_percent")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let layer_current = print_data
        .get("layer_num")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let layer_total = print_data
        .get("total_layer_num")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let job_name = print_data
        .get("subtask_name")
        .or_else(|| print_data.get("gcode_file"))
        .and_then(|v| v.as_str())
        .map(|s| s.replace(".3mf", ""))
        .unwrap_or_default();

    let nozzle_temp = print_data
        .get("nozzle_temper")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let nozzle_target = print_data
        .get("nozzle_target_temper")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let bed_temp = print_data
        .get("bed_temper")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let bed_target = print_data
        .get("bed_target_temper")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let live_speed = print_data
        .get("fan_gear")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let speed_lvl = print_data
        .get("spd_lvl")
        .and_then(|v| v.as_i64())
        .map(|n| n.to_string())
        .unwrap_or_else(|| "100".to_string());

    let lights_vec: Vec<&serde_json::Value> = print_data
        .get("lights_report")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().collect())
        .unwrap_or_default();
    let light = lights_vec
        .iter()
        .filter_map(|l| l.as_object())
        .find(|l| l.get("node").and_then(|v| v.as_str()) == Some("chamber_light"))
        .and_then(|l| l.get("mode").and_then(|v| v.as_str()))
        .unwrap_or("off");

    // AMS
    let mut ams = HashMap::new();
    if let Some(ams_data) = print_data.get("ams").and_then(|v| v.as_object()) {
        if let Some(ams_list) = ams_data.get("ams").and_then(|v| v.as_array()) {
            for slot in ams_list.iter().filter_map(|v| v.as_object()).take(4) {
                let slot_id = slot.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                if let Some(trays) = slot.get("tray").and_then(|v| v.as_array()) {
                    if let Some(tray) = trays.first().and_then(|v| v.as_object()) {
                        let color = tray
                            .get("tray_color")
                            .and_then(|v| v.as_str())
                            .map(parse_hex_color)
                            .unwrap_or_else(|| "N/A".into());
                        let material = tray
                            .get("tray_type")
                            .and_then(|v| v.as_str())
                            .unwrap_or("N/A")
                            .to_string();
                        let remaining =
                            tray.get("remain").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                        ams.insert(
                            format!("slot{}", slot_id),
                            AmsSlot { color, material, remaining },
                        );
                    }
                }
            }
        }
    }

    // Current filament
    let mut filament_type = String::new();
    if let Some(ams_data) = print_data.get("ams").and_then(|v| v.as_object()) {
        let tray_now = ams_data.get("tray_now").and_then(|v| v.as_str());
        let tray_tar = ams_data.get("tray_tar").and_then(|v| v.as_str());
        if let (Some(now), Some(tar)) = (tray_now, tray_tar) {
            if let Some(ams_list) = ams_data.get("ams").and_then(|v| v.as_array()) {
                for slot in ams_list.iter().filter_map(|v| v.as_object()) {
                    if slot.get("id").and_then(|v| v.as_str()) == Some(now) {
                        if let Some(trays) = slot.get("tray").and_then(|v| v.as_array()) {
                            for tray in trays.iter().filter_map(|v| v.as_object()) {
                                if tray.get("id").and_then(|v| v.as_str()) == Some(tar) {
                                    filament_type = tray
                                        .get("tray_type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    break;
                                }
                            }
                        }
                        break;
                    }
                }
            }
        }
    }

    let last_update = Local::now().format("%H:%M:%S").to_string();

    if is_full {
        state.mode = "printer".into();
        state.action = print_data
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        state.gcode_state = gcode_state.into();
        state.progress = progress;
        state.remaining_time = remaining_time;
        state.layer_current = layer_current;
        state.layer_total = layer_total;
        state.nozzle_temp = (nozzle_temp * 10.0).round() / 10.0;
        state.bed_temp = (bed_temp * 10.0).round() / 10.0;
        state.nozzle_target = nozzle_target;
        state.bed_target = bed_target;
        state.live_speed = live_speed;
        state.speed = speed_lvl;
        state.filament_type = filament_type;
        state.ams = ams;
        state.job_name = job_name;
        state.light = light.into();
        state.online = true;
        state.last_update = last_update;
    } else {
        state.online = true;
        state.last_update = last_update;
        if nozzle_temp > 0.0 {
            state.nozzle_temp = (nozzle_temp * 10.0).round() / 10.0;
        }
        if bed_temp > 0.0 {
            state.bed_temp = (bed_temp * 10.0).round() / 10.0;
        }
        if nozzle_target > 0.0 {
            state.nozzle_target = nozzle_target;
        }
        if bed_target > 0.0 {
            state.bed_target = bed_target;
        }
        if remaining_time > 0 {
            state.remaining_time = remaining_time;
        }
    }
}


// ── TLS: Accept any certificate (for self-signed printer certs) ──────────────

#[derive(Debug)]
struct AcceptAnyCertificate;

impl AcceptAnyCertificate {
    fn new() -> Self {
        AcceptAnyCertificate
    }
}

impl ServerCertVerifier for AcceptAnyCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        // Support all common signature schemes for Bambu Lab printer TLS
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

// ── MQTT loop (async) ────────────────────────────────────────────────────────

pub async fn mqtt_loop(app_state: Arc<SharedState>) {
    eprintln!("[MQTT] mqtt_loop started, waiting for connection signal...");
    let RUNNING: AtomicBool = AtomicBool::new(true);
    let running = &RUNNING;

    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }

        // Check if we need to reconnect (config changed or initial connection)
        if !MQTT_RECONNECT_NEEDED.load(Ordering::SeqCst) {
            // Config hasn't changed and we're already connected, just wait for signal
            tokio::time::sleep(Duration::from_millis(500)).await;
            continue;
        }

        // Reset the flag before attempting connection
        // This prevents re-entry if another config update happens during connection
        // If connection fails, we'll set it back to true after the retry delay
        MQTT_RECONNECT_NEEDED.store(false, Ordering::SeqCst);

        let (host, serial, access_code) = {
            let cfg = app_state.config.read().unwrap();
            (cfg.host.clone(), cfg.serial.clone(), cfg.access_code.clone())
        };

        // If config is empty, wait until user provides it
        if host.is_empty() || serial.is_empty() {
            eprintln!("[MQTT] Waiting for printer config (host='{}', serial='{}')", host, serial);
            tokio::time::sleep(Duration::from_secs(2)).await;
            // Re-set the flag so we check again when user enters config
            MQTT_RECONNECT_NEEDED.store(true, Ordering::SeqCst);
            continue;
        }

        eprintln!("[MQTT] Connecting to {}:8883 (serial={}, access_code present={}, tls=rustls)",
            host, serial, !access_code.is_empty());

        let mut mqtt_opts = MqttOptions::new(
            format!("bambu-monitor-{}", uuid::Uuid::new_v4()),
            &host,
            8883,
        );
        mqtt_opts.set_keep_alive(Duration::from_secs(30));
        mqtt_opts.set_clean_session(true);
        // Connection timeout is handled by the eventloop.poll() timeout, not MqttOptions
        if !access_code.is_empty() {
            mqtt_opts.set_credentials("bblp", &access_code);
        }

        // Bambu Lab printers use self-signed TLS certificates with TLS 1.2
        // Use rustls with dangerous configuration that accepts any cert
        let tls_config = rustls::ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS12])
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCertificate::new()) as Arc<dyn ServerCertVerifier>)
            .with_no_client_auth();
        mqtt_opts.set_transport(Transport::tls_with_config(
            TlsConfiguration::Rustls(Arc::new(tls_config))
        ));

        let (client, mut eventloop) = AsyncClient::new(mqtt_opts, 100);

        let report_topic = format!("device/{}/report", serial);
        let req_topic = format!("device/{}/request", serial);

        log::info!("[MQTT] Attempting to subscribe to: {}", report_topic);
        if let Err(e) = client.subscribe(&report_topic, QoS::AtMostOnce).await {
            log::error!("[MQTT] Subscribe failed: {}", e);
        } else {
            log::info!("[MQTT] Subscribed to {}", report_topic);
        }

        // Request initial pushall
        let _ = client
            .publish(&req_topic, QoS::AtMostOnce, false,
                r#"{"pushing":{"sequence_id":"0","command":"pushall"}}"#)
            .await;
        let _ = client
            .publish(&req_topic, QoS::AtMostOnce, false,
                r#"{"info":{"sequence_id":"0","command":"get_version"}}"#)
            .await;

        loop {
            match eventloop.poll().await {
                Ok(MqttEvent::Incoming(Packet::Publish(p))) => {
                    log::debug!("[MQTT] Received publish ({} bytes), topic: {}", p.payload.len(), p.topic);
                    if let Ok(obj) =
                        serde_json::from_slice::<serde_json::Value>(p.payload.as_ref())
                    {
                        let mut state = app_state.state.write().unwrap();
                        process_mqtt_payload(&obj, &mut *state);
                        state.online = true;
                        let new_state = (*state).clone();
                        drop(state);
                        let _ = app_state.tx.send(new_state);
                    }
                }
                Ok(MqttEvent::Incoming(Packet::ConnAck(_))) => {
                    eprintln!("[MQTT] Connected successfully ✓");
                    MQTT_CONNECTED.store(true, Ordering::SeqCst);
                    let state = (*app_state.state.read().unwrap()).clone();
                    let _ = app_state.tx.send(state);
                }
                Ok(MqttEvent::Incoming(Packet::SubAck(_))) => {
                    eprintln!("[MQTT] Subscribe acknowledged ✓");
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("[MQTT] Connection error: {}", e);
                    break;
                }
            }
        }

        log::info!("MQTT reconnecting in 5s …");
        {
            let mut state = app_state.state.write().unwrap();
            state.online = false;
            let _ = app_state.tx.send((*state).clone());
        }
        
        // Set reconnect flag so we attempt to reconnect with potentially updated config
        MQTT_RECONNECT_NEEDED.store(true, Ordering::SeqCst);
        
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

// ── HTTP server (sync, runs in a dedicated thread) ───────────────────────────

fn handle_connection(
    mut stream: std::net::TcpStream,
    shared: &Arc<SharedState>,
) -> std::io::Result<()> {
    use std::io::{BufRead, BufReader, Read, Write};

    let mut reader = BufReader::new(&mut stream);

    // Read request line
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    if parts.len() < 2 {
        return Ok(());
    }
    let method = parts[0];
    let path = parts[1];

    // Read headers and capture Content-Length
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line_lower = line.to_lowercase();
        if line_lower.starts_with("content-length:") {
            let val = line_lower.split(':').nth(1).unwrap_or("").trim();
            if let Ok(len) = val.parse::<usize>() {
                content_length = len;
            }
        }
        if line.trim().is_empty() {
            break;
        }
    }

    // Route
    match (path, method) {
        ("/api/health", "GET") => {
            let body = r#"{"status":"ok"}"#;
            write_http_ok(&mut stream, body.as_bytes(), "application/json")?;
        }
        ("/api/health", "OPTIONS") | ("/api/status", "OPTIONS") | 
        ("/api/config", "OPTIONS") | ("/events", "OPTIONS") => {
            write_http_options(&mut stream)?;
        }
        ("/" | "/index.html", "GET") => {
            serve_index_sync(&mut stream, shared)?;
        }
        ("/api/status", "GET") => {
            serve_status_sync(&mut stream, shared)?;
        }
        ("/api/config", "GET") => {
            serve_config_get_sync(&mut stream, shared)?;
        }
        ("/api/config", "POST") => {
            if content_length == 0 {
                let resp = b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n";
                stream.write_all(resp)?;
            } else {
                let mut body = vec![0u8; content_length];
                let mut bytes_read = 0;
                while bytes_read < content_length {
                    match stream.read(&mut body[bytes_read..]) {
                        Ok(0) => break,
                        Ok(n) => bytes_read += n,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(10));
                        }
                        Err(e) => return Err(e),
                    }
                }
                body.truncate(bytes_read);
                if let Err(e) = serve_config_post_sync(&mut stream, shared, &body) {
                    log::error!("serve_config_post_sync error: {}", e);
                }
            }
        }
        ("/events", "GET") => {
            drop(reader);
            serve_sse_sync(stream, shared)?;
            return Ok(());
        }
        _ => {
            let resp = b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\n\r\nNot Found";
            stream.write_all(resp)?;
        }
    }

    Ok(())
}

fn serve_http(listener: std::net::TcpListener, shared: Arc<SharedState>) {
    let bound_port = listener.local_addr().map(|a| a.port()).unwrap_or(5001);
    log::info!("HTTP listening on http://0.0.0.0:{}", bound_port);
    listener.set_nonblocking(true).ok();

    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                // Set stream back to blocking for reliable reads/writes.
                // Listener is non-blocking but individual connections should block.
                let _ = stream.set_nonblocking(false);
                let shared = shared.clone();
                thread::spawn(move || {
                    if let Err(e) = handle_connection(stream, &shared) {
                        log::error!("handle_connection error: {}", e);
                    }
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                log::error!("TCP accept error: {}", e);
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
}


// ── HTTP response helpers (raw TCP) ─────────────────────────────────────────

const CORS_HEADERS: &str = "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\n";

fn write_http_ok<W: std::io::Write>(
    stream: &mut W,
    body: &[u8],
    content_type: &str,
) -> std::io::Result<()> {
    let len = body.len();
    write!(stream,
        "HTTP/1.1 200 OK\r\n\
        Content-Type: {}\r\n\
        Content-Length: {}\r\n\
        Cache-Control: no-cache\r\n\
        Connection: close\r\n\
        {}\
        \r\n"
    , content_type, len, CORS_HEADERS)?;
    stream.write_all(body)?;
    stream.flush()
}

fn write_http_json_ok<W: std::io::Write>(stream: &mut W, body: &[u8]) -> std::io::Result<()> {
    write_http_ok(stream, body, "application/json")
}

fn write_http_options<W: std::io::Write>(stream: &mut W) -> std::io::Result<()> {
    write!(stream,
        "HTTP/1.1 204 No Content\r\n\
        Access-Control-Allow-Origin: *\r\n\
        Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
        Access-Control-Allow-Headers: Content-Type\r\n\
        Access-Control-Max-Age: 86400\r\n\
        Content-Length: 0\r\n\
        Connection: close\r\n\
        \r\n"
    )?;
    stream.flush()
}

fn serve_index_sync<W: std::io::Read + std::io::Write>(
    stream: &mut W,
    _shared: &Arc<SharedState>,
) -> std::io::Result<()> {
    let candidates = [
        "index.html", 
        "../index.html", 
        "dist/index.html",
        "../../index.html",
        "../../dist/index.html",
    ];
    let content = candidates
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|e| {
                    let exe_path = e.parent()?.to_path_buf();
                    let search_paths = [
                        exe_path.join("index.html"),
                        exe_path.join("dist/index.html"),
                        exe_path.join("..").join("index.html"),
                        exe_path.join("..").join("dist").join("index.html"),
                        exe_path.join("..").join("..").join("index.html"),
                        exe_path.join("..").join("..").join("dist").join("index.html"),
                        exe_path.join("Resources").join("index.html"),
                        exe_path.join("Resources").join("dist").join("index.html"),
                    ];
                    for path in &search_paths {
                        if path.exists() {
                            return std::fs::read_to_string(path).ok();
                        }
                    }
                    None
                })
        });

    match content {
        Some(c) => {
            let bytes = c.into_bytes();
            let len = bytes.len();
            write!(stream,
                "HTTP/1.1 200 OK\r\n\
                Content-Type: text/html; charset=utf-8\r\n\
                Content-Length: {}\r\n\r\n"
            , len)?;
            stream.write_all(&bytes)?;
            stream.flush()
        }
        None => {
            let body = b"Not Found";
            write!(stream,
                "HTTP/1.1 404 Not Found\r\n\
                Content-Type: text/plain\r\n\
                Content-Length: 9\r\n\r\n"
            )?;
            stream.write_all(body)?;
            stream.flush()
        }
    }
}

fn serve_status_sync<W: std::io::Read + std::io::Write>(
    stream: &mut W,
    shared: &Arc<SharedState>,
) -> std::io::Result<()> {
    let state = shared.state.read().unwrap();
    let body = serde_json::to_string(&*state).unwrap_or_default();
    drop(state);
    write_http_json_ok(stream, body.as_bytes())
}

fn serve_config_get_sync<W: std::io::Read + std::io::Write>(
    stream: &mut W,
    shared: &Arc<SharedState>,
) -> std::io::Result<()> {
    let cfg = shared.config.read().unwrap();
    let body = serde_json::to_string(&serde_json::json!({
        "host": cfg.host,
        "serial": cfg.serial,
        "has_access_code": !cfg.access_code.is_empty()
    }))
    .unwrap_or_default();
    drop(cfg);
    write_http_json_ok(stream, body.as_bytes())
}

fn serve_config_post_sync<W: std::io::Read + std::io::Write>(
    stream: &mut W,
    shared: &Arc<SharedState>,
    body: &[u8],
) -> std::io::Result<()> {
    if body.is_empty() {
        let body = r#"{"error":"empty body"}"#;
        return write_http_ok(stream, body.as_bytes(), "application/json");
    }
    if let Ok(update) = serde_json::from_slice::<ConfigUpdate>(body) {
        let mut cfg = shared.config.write().unwrap();
        if let Some(h) = update.host { cfg.host = h; }
        if let Some(sn) = update.serial { cfg.serial = sn; }
        if let Some(ac) = update.access_code { cfg.access_code = ac; }
        
        log::info!("Config updated: host={} serial={}", cfg.host, cfg.serial);
        
        let host = cfg.host.clone();
        let serial = cfg.serial.clone();
        let access_code = cfg.access_code.clone();
        
        let resp_body = serde_json::to_string(&serde_json::json!({
            "ok": true, "host": host, "serial": serial
        }))
        .unwrap_or_default();
        drop(cfg);
        
        // Persist config in background (don't block response)
        let persisted = crate::storage::PersistedConfig { host, serial, access_code };
        std::thread::spawn(move || {
            if let Err(e) = crate::storage::save_persisted_config(&persisted) {
                log::warn!("Failed to persist config: {}", e);
            }
        });
        
        write_http_json_ok(stream, resp_body.as_bytes())
    } else {
        log::warn!("POST /api/config: invalid JSON");
        let body = r#"{"error":"invalid JSON"}"#;
        write_http_ok(stream, body.as_bytes(), "application/json")
    }
}

fn serve_sse_sync(
    mut stream: std::net::TcpStream,
    shared: &Arc<SharedState>,
) -> std::io::Result<()> {
    use std::io::Write;

    // FIX: Subscribe BEFORE sending headers — this is critical!
    // The broadcast channel has capacity 64; if we send initial data before subscribing,
    // the subscriber will miss it and rx.recv() will block forever waiting for next event.
    let mut rx = shared.tx.subscribe();

    // Send current state immediately
    let state = shared.state.read().unwrap();
    let init = format!("data: {}\n\n", serde_json::to_string(&*state).unwrap_or_default());
    log::info!("[SSE] Client connected, sending initial state (online={}, gcode_state={})",
        state.online, state.gcode_state);
    drop(state);

    // SSE headers
    stream.write_all(
        b"HTTP/1.1 200 OK\r\n\
        Content-Type: text/event-stream; charset=utf-8\r\n\
        Cache-Control: no-cache\r\n\
        Connection: keep-alive\r\n\
        Access-Control-Allow-Origin: *\r\n\
        X-Accel-Buffering: no\r\n\r\n"
    )?;
    stream.flush()?;

    // Send initial state
    stream.write_all(init.as_bytes())?;
    stream.flush()?;

    // FIX: Use a longer read timeout to avoid frequent WouldBlock errors
    // and don't treat read timeouts as fatal
    stream.set_read_timeout(Some(Duration::from_secs(30))).ok();

    // Keep-alive interval
    let keepalive_interval = Duration::from_secs(25);
    let mut last_keepalive = std::time::Instant::now();

    // Small buffer for draining client data
    let mut buf = [0u8; 256];

    loop {
        // FIX #1: Use try_recv correctly — tokio broadcast TryRecvError has ONLY two variants:
        //   Ok(state)      → got a new message, send it to client
        //   Err(Lagged(n))  → n=0 means "no new data" (normal), n>0 means we fell behind (warn)
        //   Err(Closed)     → sender dropped, connection should close
        match rx.try_recv() {
            Ok(new_state) => {
                let data = format!("data: {}\n\n", serde_json::to_string(&new_state).unwrap_or_default());
                if stream.write_all(data.as_bytes()).is_err() {
                    log::info!("[SSE] Client disconnected (write error)");
                    break;
                }
                if stream.flush().is_err() {
                    log::info!("[SSE] Client disconnected (flush error)");
                    break;
                }
            }
            Err(broadcast::error::TryRecvError::Lagged(_)) => {
                // Lagged behind — we'll catch up on next iteration
            }
            Err(broadcast::error::TryRecvError::Closed) => {
                log::info!("[SSE] Broadcast channel closed, closing connection");
                break;
            }
            Err(broadcast::error::TryRecvError::Empty) => {
                // No new messages yet — normal for SSE long-poll, continue to keep-alive
            }
        }

        // Keep-alive ping every 25s to prevent proxies from dropping the connection
        if last_keepalive.elapsed() > keepalive_interval {
            last_keepalive = std::time::Instant::now();
            if stream.write_all(b": ping\n\n").is_err() {
                log::info!("[SSE] Client disconnected during keep-alive");
                break;
            }
            if stream.flush().is_err() {
                log::info!("[SSE] Flush failed during keep-alive");
                break;
            }
        }

        // FIX #2: Properly handle WouldBlock from read timeout
        // A read timeout is NOT an error — it just means the client hasn't sent anything.
        // We need to drain any pending client data to detect disconnections,
        // but ShouldBlock is expected when there's nothing to read.
        match stream.read(&mut buf) {
            Ok(0) => {
                // EOF — client closed the connection cleanly
                log::info!("[SSE] Client disconnected (EOF)");
                break;
            }
            Ok(_n) => {
                // Client sent some data (e.g., comment/heartbeat), ignore it
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Read timeout expired — this is completely normal for SSE connections.
                // Just continue the main loop to check for new broadcast messages.
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                // Another form of timeout — also normal
                continue;
            }
            Err(e) => {
                // Real I/O error — connection is broken
                log::warn!("[SSE] Read error ({:?}), closing: {}", e.kind(), e);
                break;
            }
        }
    }

    log::info!("[SSE] Connection ended normally");
    Ok(())
}

// ── HTTP server entry point (runs in a dedicated thread) ─────────────────────

pub fn start_http_server(shared: Arc<SharedState>) -> JoinHandle<()> {
    let listener = (5001u16..=5003)
        .find_map(|port| {
            std::net::TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).ok()
        });

    let Some(listener) = listener else {
        log::error!("All HTTP ports (5001-5003) are in use. Backend will not be accessible.");
        return thread::spawn(move || {});
    };

    let bound_port = listener.local_addr().map(|a| a.port()).unwrap_or(5001);
    // Set HTTP_PORT HERE so frontend sees it immediately, before thread even starts
    HTTP_PORT.store(bound_port, Ordering::SeqCst);
    log::info!("HTTP server starting on port {}", bound_port);

    thread::spawn(move || {
        serve_http(listener, shared);
    })
}

/// Read the current HTTP server port (0 if not started yet).
pub fn http_port() -> u16 {
    HTTP_PORT.load(Ordering::SeqCst)
}

/// Read whether MQTT has successfully connected at least once.
pub fn mqtt_connected() -> bool {
    MQTT_CONNECTED.load(Ordering::SeqCst)
}

/// Global shared state for config updates
pub static GLOBAL_SHARED: Lazy<Mutex<Option<Arc<SharedState>>>> = Lazy::new(|| Mutex::new(None));

/// Notify the server that config has been updated
pub fn notify_config_update() {
    log::info!("notify_config_update called - triggering MQTT reconnect");
    
    // Signal MQTT loop to reconnect with new credentials
    MQTT_RECONNECT_NEEDED.store(true, Ordering::SeqCst);
    
    match GLOBAL_SHARED.lock() {
        Ok(shared_opt) => {
            if let Some(shared) = shared_opt.as_ref() {
                let persisted = crate::storage::load_persisted_config();
                match shared.config.write() {
                    Ok(mut cfg) => {
                        let old_host = cfg.host.clone();
                        let old_serial = cfg.serial.clone();
                        
                        cfg.host = persisted.host;
                        cfg.serial = persisted.serial;
                        cfg.access_code = persisted.access_code;
                        
                        log::info!("Config updated in memory from storage: host={}→{} serial={}→{}", 
                            old_host, cfg.host, old_serial, cfg.serial);
                        
                        if old_host != cfg.host || old_serial != cfg.serial || !cfg.access_code.is_empty() {
                            log::info!("Config changed significantly, forcing MQTT reconnect");
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to lock config: {}", e);
                    }
                }
            } else {
                log::warn!("GLOBAL_SHARED not initialized yet");
            }
        }
        Err(e) => {
            log::error!("Failed to lock GLOBAL_SHARED: {}", e);
        }
    }
}
