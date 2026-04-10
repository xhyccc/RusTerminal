use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::time::{interval, Duration};

use crate::notifier::send_alert;

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

/// Background task: poll strategies.json every 60 seconds, emit heartbeat events
/// and fire notifications for enabled strategies.
async fn market_loop(app_handle: AppHandle, running: Arc<AtomicBool>) {
    let mut ticker = interval(Duration::from_secs(60));

    while running.load(Ordering::Relaxed) {
        ticker.tick().await;

        let strategies = read_strategies();
        let active: Vec<&Strategy> = strategies.iter().filter(|s| s.enabled).collect();

        let heartbeat = serde_json::json!({
            "timestamp": chrono_now(),
            "active_strategies": active.len(),
        });
        app_handle
            .emit("market-heartbeat", heartbeat)
            .unwrap_or_default();

        for strat in active {
            let title = "AI Quant Terminal";
            let body = format!("策略监控中: {}", &strat.idea[..strat.idea.len().min(60)]);
            // Only notify the first time as a demo; real logic would check price conditions.
            if strat.last_trigger.is_none() {
                send_alert(title, &body);
            }
        }
    }
}

fn chrono_now() -> String {
    // Use std time to avoid adding chrono dependency
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{}", secs)
}

#[tauri::command]
pub async fn start_market_engine(app_handle: AppHandle) -> Result<String, String> {
    let running = Arc::new(AtomicBool::new(true));
    let running_clone = running.clone();

    tokio::spawn(async move {
        market_loop(app_handle, running_clone).await;
    });

    Ok("Market engine started".to_string())
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
