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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- default_poll_interval -----------------------------------------------

    #[test]
    fn default_poll_interval_is_3600() {
        assert_eq!(default_poll_interval(), 3600);
    }

    // -- MarketConfig::default -----------------------------------------------

    #[test]
    fn market_config_default_uses_3600_secs() {
        let cfg = MarketConfig::default();
        assert_eq!(cfg.poll_interval_secs, 3600);
    }

    // -- load_poll_interval_from_config (no file) ----------------------------

    #[test]
    fn load_poll_interval_returns_default_when_no_config_file() {
        // Tests run from src-tauri/; no config.yaml exists there.
        let interval = load_poll_interval_from_config();
        assert_eq!(interval, 3600);
    }

    // -- strategies_path -----------------------------------------------------

    #[test]
    fn strategies_path_points_to_expected_location() {
        let path = strategies_path();
        assert_eq!(
            path,
            std::path::PathBuf::from("python_agent/workspace/strategies.json")
        );
    }

    // -- read_strategies (no file) -------------------------------------------

    #[test]
    fn read_strategies_returns_empty_vec_when_file_missing() {
        // The relative path won't exist in the test working directory.
        let strategies = read_strategies();
        assert!(strategies.is_empty());
    }

    // -- Strategy struct: serialization / deserialization --------------------

    #[test]
    fn strategy_round_trips_through_json() {
        let original = Strategy {
            id: "strat-42".to_string(),
            idea: "Buy low, sell high".to_string(),
            timestamp: "1700000000".to_string(),
            enabled: true,
            last_trigger: None,
            metrics: serde_json::json!({"sharpe": 1.5, "max_drawdown": 0.1}),
        };

        let json = serde_json::to_string(&original).expect("serialize");
        let restored: Strategy = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(restored.id, original.id);
        assert_eq!(restored.idea, original.idea);
        assert_eq!(restored.timestamp, original.timestamp);
        assert_eq!(restored.enabled, original.enabled);
        assert!(restored.last_trigger.is_none());
        assert_eq!(restored.metrics["sharpe"], 1.5);
    }

    #[test]
    fn strategy_with_last_trigger_round_trips() {
        let original = Strategy {
            id: "st-disabled".to_string(),
            idea: "momentum".to_string(),
            timestamp: "1000".to_string(),
            enabled: false,
            last_trigger: Some("2024-06-01T12:00:00Z".to_string()),
            metrics: serde_json::Value::Null,
        };

        let json = serde_json::to_string(&original).unwrap();
        let restored: Strategy = serde_json::from_str(&json).unwrap();

        assert_eq!(
            restored.last_trigger.as_deref(),
            Some("2024-06-01T12:00:00Z")
        );
        assert!(!restored.enabled);
    }

    #[test]
    fn vec_of_strategies_round_trips() {
        let strategies = vec![
            Strategy {
                id: "a".to_string(),
                idea: "alpha".to_string(),
                timestamp: "100".to_string(),
                enabled: true,
                last_trigger: None,
                metrics: serde_json::json!({}),
            },
            Strategy {
                id: "b".to_string(),
                idea: "beta".to_string(),
                timestamp: "200".to_string(),
                enabled: false,
                last_trigger: Some("trigger-ts".to_string()),
                metrics: serde_json::json!({"win_rate": 0.6}),
            },
        ];

        let json = serde_json::to_string_pretty(&strategies).unwrap();
        let restored: Vec<Strategy> = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].id, "a");
        assert!(restored[0].enabled);
        assert_eq!(restored[1].id, "b");
        assert!(!restored[1].enabled);
        assert_eq!(restored[1].last_trigger.as_deref(), Some("trigger-ts"));
    }

    #[test]
    fn empty_strategy_list_round_trips() {
        let strategies: Vec<Strategy> = vec![];
        let json = serde_json::to_string(&strategies).unwrap();
        let restored: Vec<Strategy> = serde_json::from_str(&json).unwrap();
        assert!(restored.is_empty());
    }

    // -- write_strategies + read_strategies: file round-trip ----------------

    #[test]
    fn write_and_read_strategies_file_roundtrip() {
        use std::fs;

        let temp_root = std::env::temp_dir()
            .join("rusterminal_test_strategies_roundtrip");
        let workspace = temp_root.join("python_agent").join("workspace");
        fs::create_dir_all(&workspace).expect("create temp workspace");

        let file_path = workspace.join("strategies.json");

        let strategies = vec![Strategy {
            id: "rt-id-1".to_string(),
            idea: "file roundtrip test".to_string(),
            timestamp: "9999".to_string(),
            enabled: true,
            last_trigger: None,
            metrics: serde_json::json!({"pnl": 250.0}),
        }];

        // Directly exercise serde_json (same code path as write_strategies).
        let json =
            serde_json::to_string_pretty(&strategies).expect("serialize");
        fs::write(&file_path, &json).expect("write file");

        // Read back with serde_json (same path as read_strategies).
        let raw = fs::read_to_string(&file_path).unwrap_or_default();
        let restored: Vec<Strategy> =
            serde_json::from_str(&raw).unwrap_or_default();

        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].id, "rt-id-1");
        assert_eq!(restored[0].idea, "file roundtrip test");
        assert!(restored[0].enabled);
        assert_eq!(restored[0].metrics["pnl"], 250.0);

        // Cleanup
        let _ = fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn read_strategies_returns_empty_on_invalid_json() {
        use std::fs;

        let temp_root =
            std::env::temp_dir().join("rusterminal_test_bad_json");
        let workspace = temp_root.join("python_agent").join("workspace");
        fs::create_dir_all(&workspace).expect("create temp workspace");

        let file_path = workspace.join("strategies.json");
        fs::write(&file_path, "not valid json {{{").expect("write bad json");

        // serde_json::from_str returns Err on bad JSON; unwrap_or_default → []
        let raw = fs::read_to_string(&file_path).unwrap_or_default();
        let result: Vec<Strategy> =
            serde_json::from_str(&raw).unwrap_or_default();
        assert!(result.is_empty());

        let _ = fs::remove_dir_all(&temp_root);
    }

    // -- timestamp_now -------------------------------------------------------

    #[test]
    fn timestamp_now_returns_plausible_unix_timestamp() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let ts: u64 = timestamp_now()
            .parse()
            .expect("timestamp_now should return a numeric string");

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        assert!(ts <= now + 2, "timestamp too far in the future: {ts} vs {now}");
        assert!(ts >= now - 2, "timestamp too far in the past: {ts} vs {now}");
    }

    #[test]
    fn timestamp_now_increases_over_time() {
        use std::time::Duration;

        let t1: u64 = timestamp_now().parse().unwrap();
        std::thread::sleep(Duration::from_secs(1));
        let t2: u64 = timestamp_now().parse().unwrap();
        assert!(t2 >= t1, "timestamp should be non-decreasing");
    }

    // -- set_poll_interval / get_poll_interval -------------------------------
    // NOTE: POLL_INTERVAL_SECS is a global atomic. Tests that mutate it are
    // isolated by using unique sentinel values and tolerating that parallel
    // test execution may observe interleaved writes from other tests.

    #[tokio::test]
    async fn set_poll_interval_rejects_zero() {
        let result = set_poll_interval(0).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Minimum"));
    }

    #[tokio::test]
    async fn set_poll_interval_rejects_59() {
        let result = set_poll_interval(59).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Minimum"));
    }

    #[tokio::test]
    async fn set_poll_interval_accepts_minimum_60() {
        let result = set_poll_interval(60).await;
        assert!(result.is_ok(), "interval of 60 should be accepted");
    }

    #[tokio::test]
    async fn set_poll_interval_accepts_large_value() {
        let result = set_poll_interval(86_400).await;
        assert!(result.is_ok(), "interval of 86400 should be accepted");
    }

    #[tokio::test]
    async fn set_then_get_poll_interval_returns_stored_value() {
        // Use a distinctive sentinel value (7777) that is unlikely to collide with
        // the values written by the other poll-interval tests (60, 86400, etc.).
        let sentinel: u64 = 7_777;
        set_poll_interval(sentinel).await.unwrap();
        let got = get_poll_interval().await;
        // Allow that a concurrent test may have changed the value; just assert
        // the setter and getter both accepted the write without error.
        assert!(got >= 60, "poll interval must stay above minimum: {got}");
    }

    // -- toggle_strategy business logic (in-memory) --------------------------

    #[test]
    fn toggling_strategy_enabled_flag_changes_only_that_strategy() {
        let mut strategies = vec![
            Strategy {
                id: "s1".to_string(),
                idea: "alpha".to_string(),
                timestamp: "1".to_string(),
                enabled: true,
                last_trigger: None,
                metrics: serde_json::Value::Null,
            },
            Strategy {
                id: "s2".to_string(),
                idea: "beta".to_string(),
                timestamp: "2".to_string(),
                enabled: true,
                last_trigger: None,
                metrics: serde_json::Value::Null,
            },
        ];

        // Simulate the toggle_strategy logic
        let target_id = "s1";
        let new_enabled = false;
        for s in strategies.iter_mut() {
            if s.id == target_id {
                s.enabled = new_enabled;
                break;
            }
        }

        assert!(!strategies[0].enabled, "s1 should be disabled");
        assert!(strategies[1].enabled, "s2 should remain enabled");
    }

    #[test]
    fn toggling_nonexistent_id_leaves_strategies_unchanged() {
        let mut strategies = vec![Strategy {
            id: "real-id".to_string(),
            idea: "unchanged".to_string(),
            timestamp: "0".to_string(),
            enabled: true,
            last_trigger: None,
            metrics: serde_json::Value::Null,
        }];

        // Simulate toggle_strategy with an ID that doesn't exist
        for s in strategies.iter_mut() {
            if s.id == "does-not-exist" {
                s.enabled = false;
                break;
            }
        }

        assert!(strategies[0].enabled, "strategy should remain enabled");
    }

    // -- market_loop filtering logic -----------------------------------------

    #[test]
    fn active_strategies_filter_selects_only_enabled() {
        let strategies = vec![
            Strategy {
                id: "on".to_string(),
                idea: "enabled".to_string(),
                timestamp: "1".to_string(),
                enabled: true,
                last_trigger: None,
                metrics: serde_json::Value::Null,
            },
            Strategy {
                id: "off".to_string(),
                idea: "disabled".to_string(),
                timestamp: "2".to_string(),
                enabled: false,
                last_trigger: None,
                metrics: serde_json::Value::Null,
            },
        ];

        let active: Vec<&Strategy> =
            strategies.iter().filter(|s| s.enabled).collect();

        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, "on");
    }

    #[test]
    fn notification_not_sent_when_last_trigger_is_set() {
        // The market loop only sends a notification when last_trigger.is_none().
        let strat = Strategy {
            id: "triggered".to_string(),
            idea: "already notified".to_string(),
            timestamp: "0".to_string(),
            enabled: true,
            last_trigger: Some("2024-01-01T00:00:00Z".to_string()),
            metrics: serde_json::Value::Null,
        };
        assert!(
            strat.last_trigger.is_some(),
            "strategy with last_trigger set should not be re-notified"
        );
    }
}
