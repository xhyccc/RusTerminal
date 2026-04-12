use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::time::{sleep, Duration};

use crate::notifier::send_alert;

/// Global poll interval — readable/writable from any thread without a mutex.
/// Default 3600 s (1 hour), suitable for non-high-frequency use.
/// Updated via the `set_poll_interval` Tauri command.
static POLL_INTERVAL_SECS: AtomicU64 = AtomicU64::new(3600);

// ---------------------------------------------------------------------------
// Config loading (market section of config.yaml)
// ---------------------------------------------------------------------------

#[derive(Debug, Default, serde::Deserialize)]
struct MarketAppConfig {
    #[serde(default)]
    market: MarketConfig,
}

#[derive(Debug, serde::Deserialize)]
struct MarketConfig {
    #[serde(default = "default_poll_interval")]
    poll_interval_secs: u64,
}

fn default_poll_interval() -> u64 {
    3600
}

impl Default for MarketConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: default_poll_interval(),
        }
    }
}

/// Read `market.poll_interval_secs` from config.yaml (project root).
/// Returns the default (3600) on any error so startup is never blocked.
fn load_poll_interval_from_config() -> u64 {
    let path = std::path::Path::new("config.yaml");
    if !path.exists() {
        return default_poll_interval();
    }
    match std::fs::read_to_string(path) {
        Ok(text) => {
            serde_yaml::from_str::<MarketAppConfig>(&text)
                .unwrap_or_default()
                .market
                .poll_interval_secs
        }
        Err(_) => default_poll_interval(),
    }
}

// ---------------------------------------------------------------------------
// Strategy data
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Strategy {
    pub id: String,
    pub idea: String,
    pub timestamp: String,
    pub enabled: bool,
    pub last_trigger: Option<String>,
    pub metrics: serde_json::Value,
}

fn strategies_path() -> PathBuf {
    PathBuf::from("python_agent/workspace/strategies.json")
}

fn read_strategies() -> Vec<Strategy> {
    let path = strategies_path();
    if !path.exists() {
        return vec![];
    }
    let raw = fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

fn write_strategies(strategies: &[Strategy]) -> Result<(), String> {
    let path = strategies_path();
    let json = serde_json::to_string_pretty(strategies)
        .map_err(|e| format!("Serialization error: {}", e))?;
    fs::write(path, json).map_err(|e| format!("Write error: {}", e))
}

// ---------------------------------------------------------------------------
// Market loop
// ---------------------------------------------------------------------------

/// Background task: sleeps for POLL_INTERVAL_SECS (checked in small ticks so
/// interval changes take effect quickly), then emits heartbeat events and
/// fires notifications for enabled strategies.
async fn market_loop(app_handle: AppHandle, running: Arc<AtomicBool>) {
    // Tick granularity: check every 5 seconds whether the full interval has
    // elapsed or the loop has been stopped.  5 s balances responsiveness (interval
    // changes take effect quickly) against needless CPU wakeups.
    const TICK_SECS: u64 = 5;
    let mut elapsed: u64 = 0;

    while running.load(Ordering::Relaxed) {
        sleep(Duration::from_secs(TICK_SECS)).await;
        elapsed += TICK_SECS;

        let target = POLL_INTERVAL_SECS.load(Ordering::Relaxed);
        if elapsed < target {
            continue;
        }
        elapsed = 0;

        let strategies = read_strategies();
        let active: Vec<&Strategy> = strategies.iter().filter(|s| s.enabled).collect();

        let heartbeat = serde_json::json!({
            "timestamp": timestamp_now(),
            "active_strategies": active.len(),
            "poll_interval_secs": target,
        });
        app_handle
            .emit("market-heartbeat", heartbeat)
            .unwrap_or_default();

        for strat in active {
            let title = "AI Quant Terminal";
            let body = format!(
                "策略监控中: {}",
                strat.idea.chars().take(60).collect::<String>()
            );
            // Only notify the first time as a demo; real logic would check price conditions.
            if strat.last_trigger.is_none() {
                send_alert(title, &body);
            }
        }
    }
}

fn timestamp_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", secs)
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn start_market_engine(app_handle: AppHandle) -> Result<String, String> {
    // Apply config.yaml value before starting the loop.
    let configured = load_poll_interval_from_config();
    POLL_INTERVAL_SECS.store(configured, Ordering::Relaxed);

    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    tokio::spawn(async move {
        market_loop(app_handle, running_clone).await;
    });

    Ok(format!(
        "Market engine started (poll interval: {}s)",
        configured
    ))
}

#[tauri::command]
pub async fn get_strategies() -> Result<Vec<Strategy>, String> {
    Ok(read_strategies())
}

#[tauri::command]
pub async fn toggle_strategy(id: String, enabled: bool) -> Result<(), String> {
    let mut strategies = read_strategies();
    for s in strategies.iter_mut() {
        if s.id == id {
            s.enabled = enabled;
            break;
        }
    }
    write_strategies(&strategies)
}

/// Get the current poll interval in seconds.
#[tauri::command]
pub async fn get_poll_interval() -> u64 {
    POLL_INTERVAL_SECS.load(Ordering::Relaxed)
}

/// Set the poll interval at runtime.  Minimum 60 seconds to avoid hammering
/// data sources.  Changes take effect within 5 seconds (the loop tick).
#[tauri::command]
pub async fn set_poll_interval(secs: u64) -> Result<(), String> {
    if secs < 60 {
        return Err("Minimum poll interval is 60 seconds".to_string());
    }
    POLL_INTERVAL_SECS.store(secs, Ordering::Relaxed);
    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;

    /// Serialize tests that mutate the process-global working directory.
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    // ── default_poll_interval ────────────────────────────────────────────────

    #[test]
    fn test_default_poll_interval() {
        assert_eq!(default_poll_interval(), 3600);
    }

    // ── MarketConfig default ─────────────────────────────────────────────────

    #[test]
    fn test_market_config_default() {
        let cfg = MarketConfig::default();
        assert_eq!(cfg.poll_interval_secs, 3600);
    }

    // ── load_poll_interval_from_config ───────────────────────────────────────

    #[test]
    fn test_load_poll_interval_missing_file() {
        // When config.yaml does not exist the default (3600) is returned.
        // We work in a temp dir that contains no config.yaml.
        let _lock = CWD_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let interval = load_poll_interval_from_config();

        std::env::set_current_dir(original).unwrap();
        assert_eq!(interval, 3600);
    }

    #[test]
    fn test_load_poll_interval_from_valid_config() {
        let _lock = CWD_LOCK.lock().unwrap();
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "market:\n  poll_interval_secs: 120").unwrap();

        // Temporarily change cwd so the function picks up our temp config.
        let tmp_dir = f.path().parent().unwrap().to_path_buf();
        let config_path = tmp_dir.join("config.yaml");
        std::fs::copy(f.path(), &config_path).unwrap();

        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp_dir).unwrap();

        let interval = load_poll_interval_from_config();

        std::env::set_current_dir(original).unwrap();
        std::fs::remove_file(config_path).unwrap();

        assert_eq!(interval, 120);
    }

    #[test]
    fn test_load_poll_interval_invalid_yaml_returns_default() {
        let _lock = CWD_LOCK.lock().unwrap();
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "not: valid: yaml: {{{{").unwrap();

        let tmp_dir = f.path().parent().unwrap().to_path_buf();
        let config_path = tmp_dir.join("config.yaml");
        std::fs::copy(f.path(), &config_path).unwrap();

        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp_dir).unwrap();

        let interval = load_poll_interval_from_config();

        std::env::set_current_dir(original).unwrap();
        std::fs::remove_file(config_path).unwrap();

        assert_eq!(interval, 3600);
    }

    // ── timestamp_now ────────────────────────────────────────────────────────

    #[test]
    fn test_timestamp_now_is_numeric_string() {
        let ts = timestamp_now();
        assert!(!ts.is_empty(), "timestamp should not be empty");
        ts.parse::<u64>().expect("timestamp should be a valid u64");
    }

    #[test]
    fn test_timestamp_now_is_reasonable() {
        let ts: u64 = timestamp_now().parse().unwrap();
        // Sanity: after 2020-01-01 (Unix 1577836800) and before year 2100.
        assert!(ts > 1_577_836_800, "timestamp too small");
        assert!(ts < 4_102_444_800, "timestamp too large");
    }

    // ── read_strategies / write_strategies ───────────────────────────────────

    #[test]
    fn test_read_strategies_missing_file_returns_empty() {
        let _lock = CWD_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let strategies = read_strategies();

        std::env::set_current_dir(original).unwrap();
        assert!(strategies.is_empty());
    }

    #[test]
    fn test_write_then_read_strategies() {
        let _lock = CWD_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("python_agent/workspace")).unwrap();

        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let strats = vec![
            Strategy {
                id: "s1".to_string(),
                idea: "buy low sell high".to_string(),
                timestamp: "1000".to_string(),
                enabled: true,
                last_trigger: None,
                metrics: serde_json::json!({}),
            },
            Strategy {
                id: "s2".to_string(),
                idea: "momentum strategy".to_string(),
                timestamp: "2000".to_string(),
                enabled: false,
                last_trigger: Some("1234".to_string()),
                metrics: serde_json::json!({"sharpe": 1.5}),
            },
        ];

        write_strategies(&strats).expect("write should succeed");
        let loaded = read_strategies();

        std::env::set_current_dir(original).unwrap();

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "s1");
        assert!(loaded[0].enabled);
        assert_eq!(loaded[1].id, "s2");
        assert!(!loaded[1].enabled);
        assert_eq!(loaded[1].last_trigger.as_deref(), Some("1234"));
    }

    // ── POLL_INTERVAL_SECS atomic ─────────────────────────────────────────────

    #[test]
    fn test_poll_interval_atomic_store_load() {
        POLL_INTERVAL_SECS.store(300, Ordering::Relaxed);
        assert_eq!(POLL_INTERVAL_SECS.load(Ordering::Relaxed), 300);
        // Restore default so other tests are unaffected.
        POLL_INTERVAL_SECS.store(3600, Ordering::Relaxed);
    }

    // ── set_poll_interval validation ─────────────────────────────────────────

    #[tokio::test]
    async fn test_set_poll_interval_rejects_below_minimum() {
        let result = set_poll_interval(59).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Minimum"));
    }

    #[tokio::test]
    async fn test_set_poll_interval_accepts_minimum() {
        let result = set_poll_interval(60).await;
        assert!(result.is_ok());
        assert_eq!(POLL_INTERVAL_SECS.load(Ordering::Relaxed), 60);
        // Restore.
        POLL_INTERVAL_SECS.store(3600, Ordering::Relaxed);
    }

    #[tokio::test]
    async fn test_set_poll_interval_large_value() {
        let result = set_poll_interval(86400).await;
        assert!(result.is_ok());
        assert_eq!(POLL_INTERVAL_SECS.load(Ordering::Relaxed), 86400);
        POLL_INTERVAL_SECS.store(3600, Ordering::Relaxed);
    }
}
