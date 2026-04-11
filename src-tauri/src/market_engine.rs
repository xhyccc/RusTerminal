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
        Self { poll_interval_secs: default_poll_interval() }
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
        Ok(text) => serde_yaml::from_str::<MarketAppConfig>(&text)
            .unwrap_or_default()
            .market
            .poll_interval_secs,
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
            let body = format!("策略监控中: {}", strat.idea.chars().take(60).collect::<String>());
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

    Ok(format!("Market engine started (poll interval: {}s)", configured))
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
