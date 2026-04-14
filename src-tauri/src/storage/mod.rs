use directories::ProjectDirs;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::RwLock;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Failed to get config directory")]
    NoConfigDir,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PrinterConfig {
    pub host: String,
    pub serial: String,
    pub access_code: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub printers: HashMap<String, PrinterConfig>,
    pub active_printer: Option<String>,
    pub settings: AppSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub check_interval: u64,
    pub notification_enabled: bool,
    pub start_minimized: bool,
    pub auto_connect: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            check_interval: 6,
            notification_enabled: true,
            start_minimized: false,
            auto_connect: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintHistoryRecord {
    pub id: Option<i64>,
    pub printer_serial: String,
    pub job_name: String,
    pub started_at: String,
    pub finished_at: String,
    pub filament_type: String,
    pub layer_count: u32,
    pub success: bool,
}

static CONFIG_DIR: Lazy<PathBuf> = Lazy::new(|| {
    ProjectDirs::from("com", "david", "bambu-monitor")
        .map(|dirs| dirs.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."))
});

static APP_CONFIG: Lazy<RwLock<AppConfig>> =
    Lazy::new(|| RwLock::new(load_config().unwrap_or_default()));

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistedConfig {
    pub host: String,
    pub serial: String,
    #[serde(rename = "access_code")]
    pub access_code: String,
}

impl From<PersistedConfig> for PrinterConfig {
    fn from(p: PersistedConfig) -> Self {
        PrinterConfig {
            host: p.host,
            serial: p.serial,
            access_code: p.access_code,
            name: String::new(),
        }
    }
}

pub fn load_persisted_config() -> PersistedConfig {
    let path = config_path();
    if !path.exists() {
        return PersistedConfig::default();
    }
    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => PersistedConfig::default(),
    }
}

pub fn save_persisted_config(config: &PersistedConfig) -> Result<(), StorageError> {
    let path = config_path();
    if let Some(dir) = path.parent() {
        if !dir.exists() {
            fs::create_dir_all(dir)?;
        }
    }
    let content = serde_json::to_string_pretty(config)?;
    fs::write(&path, content)?;
    Ok(())
}

pub fn get_config_dir() -> PathBuf {
    let dir = CONFIG_DIR.clone();
    if !dir.exists() {
        fs::create_dir_all(&dir).ok();
    }
    dir
}

pub fn config_path() -> PathBuf {
    get_config_dir().join("config.json")
}

pub fn database_path() -> PathBuf {
    get_config_dir().join("print_history.db")
}

pub fn load_config() -> Result<AppConfig, StorageError> {
    let path = config_path();
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let content = fs::read_to_string(&path)?;
    let config: AppConfig = serde_json::from_str(&content)?;
    Ok(config)
}

pub fn save_config(config: &AppConfig) -> Result<(), StorageError> {
    let path = config_path();
    let dir = path.parent().unwrap();
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }
    let content = serde_json::to_string_pretty(config)?;
    fs::write(&path, content)?;
    Ok(())
}

pub fn get_app_config() -> AppConfig {
    APP_CONFIG.read().unwrap().clone()
}

pub fn update_app_config<F>(f: F) -> Result<(), StorageError>
where
    F: FnOnce(&mut AppConfig),
{
    let mut config = APP_CONFIG.write().unwrap();
    f(&mut config);
    save_config(&config)?;
    Ok(())
}

pub fn add_printer(id: String, config: PrinterConfig) -> Result<(), StorageError> {
    update_app_config(|app| {
        app.printers.insert(id.clone(), config);
        if app.active_printer.is_none() {
            app.active_printer = Some(id);
        }
    })
}

pub fn remove_printer(id: &str) -> Result<(), StorageError> {
    update_app_config(|app| {
        app.printers.remove(id);
        if app.active_printer.as_deref() == Some(id) {
            app.active_printer = app.printers.keys().next().cloned();
        }
    })
}

pub fn set_active_printer(id: String) -> Result<(), StorageError> {
    update_app_config(|app| {
        if app.printers.contains_key(&id) {
            app.active_printer = Some(id);
        }
    })
}

pub fn get_printer_config(id: &str) -> Option<PrinterConfig> {
    APP_CONFIG.read().unwrap().printers.get(id).cloned()
}

pub fn update_printer_config(id: &str, config: PrinterConfig) -> Result<(), StorageError> {
    update_app_config(|app| {
        if app.printers.contains_key(id) {
            app.printers.insert(id.to_string(), config);
        }
    })
}

pub fn init_database() -> Result<rusqlite::Connection, StorageError> {
    let db_path = database_path();
    let dir = db_path.parent().unwrap();
    if !dir.exists() {
        fs::create_dir_all(dir)?;
    }
    let conn = rusqlite::Connection::open(&db_path)?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS print_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            printer_serial TEXT NOT NULL,
            job_name TEXT NOT NULL,
            started_at TEXT NOT NULL,
            finished_at TEXT NOT NULL,
            filament_type TEXT,
            layer_count INTEGER DEFAULT 0,
            success INTEGER DEFAULT 1
        )",
        [],
    )?;
    Ok(conn)
}

pub fn add_print_history(record: PrintHistoryRecord) -> Result<i64, StorageError> {
    let conn = init_database()?;
    conn.execute(
        "INSERT INTO print_history (printer_serial, job_name, started_at, finished_at, filament_type, layer_count, success)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            record.printer_serial,
            record.job_name,
            record.started_at,
            record.finished_at,
            record.filament_type,
            record.layer_count,
            record.success as i32
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get_print_history(limit: i32) -> Result<Vec<PrintHistoryRecord>, StorageError> {
    let conn = init_database()?;
    let mut stmt = conn.prepare(
        "SELECT id, printer_serial, job_name, started_at, finished_at, filament_type, layer_count, success
         FROM print_history ORDER BY id DESC LIMIT ?1"
    )?;
    let records = stmt.query_map([limit], |row| {
        Ok(PrintHistoryRecord {
            id: Some(row.get(0)?),
            printer_serial: row.get(1)?,
            job_name: row.get(2)?,
            started_at: row.get(3)?,
            finished_at: row.get(4)?,
            filament_type: row.get(5)?,
            layer_count: row.get(6)?,
            success: row.get::<_, i32>(7)? == 1,
        })
    })?;
    let mut result = Vec::new();
    for record in records {
        result.push(record?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_path() {
        let path = config_path();
        println!("Config path: {:?}", path);
    }
}
