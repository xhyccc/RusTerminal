use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use tauri::{AppHandle, Emitter};

// ---------------------------------------------------------------------------
// YAML config
// ---------------------------------------------------------------------------

/// Minimal mirror of config.yaml — only the fields that agent_runner needs.
#[derive(Debug, Default, serde::Deserialize)]
struct AppConfig {
    #[serde(default)]
    llm: LlmConfig,
}

#[derive(Debug, Default, serde::Deserialize)]
struct LlmConfig {
    #[serde(default)]
    provider: String,
    #[serde(default)]
    api_key: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    azure: AzureConfig,
}

#[derive(Debug, Default, serde::Deserialize)]
struct AzureConfig {
    #[serde(default)]
    endpoint: String,
    #[serde(default)]
    deployment: String,
    #[serde(default)]
    api_version: String,
}

/// Try to load `config.yaml` from the current working directory (project root).
/// Returns a default (all-empty) config on any error so the rest of the code
/// can always use env vars as the authoritative source.
fn load_app_config() -> AppConfig {
    let path = std::path::Path::new("config.yaml");
    if !path.exists() {
        return AppConfig::default();
    }
    match std::fs::read_to_string(path) {
        Ok(text) => serde_yaml::from_str(&text).unwrap_or_default(),
        Err(_) => AppConfig::default(),
    }
}

/// Locate the goose binary.  Checks common installation paths in addition to
/// whatever is on PATH so that a freshly-installed goose is found even when
/// the shell profile has not been re-sourced.
fn find_goose() -> Option<String> {
    let candidates = [
        "goose".to_string(),
        "/usr/local/bin/goose".to_string(),
        "/usr/bin/goose".to_string(),
        format!(
            "{}/.local/bin/goose",
            std::env::var("HOME").unwrap_or_default()
        ),
    ];

    for candidate in &candidates {
        if !candidate.is_empty()
            && Command::new(candidate)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok()
        {
            return Some(candidate.clone());
        }
    }
    None
}

/// Return the first non-empty value found among the given environment variable names.
fn first_env(vars: &[&str]) -> Option<String> {
    vars.iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.is_empty()))
}

/// Build the list of extra environment variables to inject into the goose process
/// so that it uses the same LLM provider / key / model as the Python fallback.
///
/// Priority (highest first):
///   1. Shell environment variables (already present in `std::env`)
///   2. config.yaml values
///
/// Reads `LLM_PROVIDER` (default `openai`) and maps to goose's own variables:
///   - `GOOSE_PROVIDER`   — which provider plugin goose should use
///   - `OPENAI_API_KEY`   — API key forwarded for OpenAI-compatible providers
///   - `OPENAI_BASE_URL`  — custom endpoint for kimi / glm / siliconflow
///   - `GOOSE_MODEL`      — model override (from `LLM_MODEL` or provider default)
///
/// Azure: goose receives `GOOSE_PROVIDER=azure`; the `AZURE_OPENAI_*` variables
/// are already in the environment and are inherited automatically.
fn llm_env_for_goose() -> Vec<(String, String)> {
    let cfg = load_app_config();

    // Helper: env var → config.yaml fallback → empty string.
    // Avoids unnecessary allocations: only trims/clones when a non-empty value exists.
    let resolve = |env_var: &str, yaml_val: &str| -> String {
        let from_env = std::env::var(env_var).unwrap_or_default();
        let trimmed_env = from_env.trim();
        if !trimmed_env.is_empty() {
            return trimmed_env.to_string();
        }
        let trimmed_yaml = yaml_val.trim();
        if !trimmed_yaml.is_empty() {
            return trimmed_yaml.to_string();
        }
        String::new()
    };

    let provider = resolve("LLM_PROVIDER", &cfg.llm.provider)
        .to_lowercase()
        .or_if_empty("openai".to_string());

    let mut env: Vec<(String, String)> = Vec::new();

    if provider == "azure" {
        env.push(("GOOSE_PROVIDER".to_string(), "azure".to_string()));
        // Inject Azure vars from config if not already in the environment.
        for (var, yaml) in [
            ("AZURE_OPENAI_API_KEY", cfg.llm.api_key.as_str()),
            ("AZURE_OPENAI_ENDPOINT", cfg.llm.azure.endpoint.as_str()),
            ("AZURE_OPENAI_DEPLOYMENT", cfg.llm.azure.deployment.as_str()),
            (
                "AZURE_OPENAI_API_VERSION",
                cfg.llm.azure.api_version.as_str(),
            ),
        ] {
            let val = resolve(var, yaml);
            if !val.is_empty() {
                env.push((var.to_string(), val));
            }
        }
        return env;
    }

    // Map provider → (goose_provider, default_base_url, api_key_vars, default_model)
    let (goose_provider, default_base, key_vars, default_model): (
        &str,
        Option<&str>,
        &[&str],
        &str,
    ) = match provider.as_str() {
        "kimi" => (
            "openai",
            Some("https://api.moonshot.cn/v1"),
            &["KIMI_API_KEY", "LLM_API_KEY", "OPENAI_API_KEY"],
            "moonshot-v1-8k",
        ),
        "glm" => (
            "openai",
            Some("https://open.bigmodel.cn/api/paas/v4"),
            &["GLM_API_KEY", "LLM_API_KEY", "OPENAI_API_KEY"],
            "glm-4-flash",
        ),
        "siliconflow" => (
            "openai",
            Some("https://api.siliconflow.cn/v1"),
            &["SILICONFLOW_API_KEY", "LLM_API_KEY", "OPENAI_API_KEY"],
            "Qwen/Qwen2.5-7B-Instruct",
        ),
        _ => (
            "openai",
            None,
            &["OPENAI_API_KEY", "LLM_API_KEY"],
            "gpt-4o-mini",
        ),
    };

    env.push(("GOOSE_PROVIDER".to_string(), goose_provider.to_string()));

    // Base URL: env var > config.yaml > provider default
    let base_url = resolve("LLM_BASE_URL", &cfg.llm.base_url)
        .or_if_empty(default_base.unwrap_or("").to_string());
    if !base_url.is_empty() {
        env.push(("OPENAI_BASE_URL".to_string(), base_url));
    }

    // API key: env var chain > config.yaml api_key
    let api_key = first_env(key_vars)
        .unwrap_or_default()
        .or_if_empty(cfg.llm.api_key.clone());
    if !api_key.is_empty() {
        env.push(("OPENAI_API_KEY".to_string(), api_key));
    }

    // Model: env var > config.yaml > provider default
    let model = resolve("LLM_MODEL", &cfg.llm.model).or_if_empty(default_model.to_string());
    env.push(("GOOSE_MODEL".to_string(), model));

    env
}

// ---------------------------------------------------------------------------
// String helper — not in std
// ---------------------------------------------------------------------------

trait OrIfEmpty {
    fn or_if_empty(self, fallback: String) -> String;
}
impl OrIfEmpty for String {
    fn or_if_empty(self, fallback: String) -> String {
        if self.is_empty() {
            fallback
        } else {
            self
        }
    }
}

/// Stream every line from a spawned process to the frontend as an `agent-log` event.
fn stream_to_frontend(app_handle: &AppHandle, child: &mut std::process::Child) {
    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        for line in reader.lines().flatten() {
            app_handle.emit("agent-log", line).unwrap_or_default();
        }
    }
}

/// Primary path: run the strategy generation via **goose** using the
/// `python_agent/quant_strategy.yaml` recipe.
async fn run_with_goose(
    app_handle: AppHandle,
    idea: String,
    goose_bin: String,
) -> Result<String, String> {
    app_handle
        .emit("agent-log", "[AGENT_THINKING] Starting goose AI agent…")
        .unwrap_or_default();

    let recipe = "python_agent/quant_strategy.yaml";
    let params = format!("idea={idea}");

    let mut child = Command::new(&goose_bin)
        .args([
            "run",
            "--recipe",
            recipe,
            "--params",
            &params,
            "--no-session",
        ])
        .envs(llm_env_for_goose())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start goose: {e}"))?;

    stream_to_frontend(&app_handle, &mut child);

    match child.wait() {
        Ok(s) if s.success() => Ok("Goose agent completed successfully".to_string()),
        Ok(_) => Err("Goose agent exited with a non-zero status".to_string()),
        Err(e) => Err(format!("Goose agent failed: {e}")),
    }
}

/// Fallback path: invoke the Python agent (`python_agent/agent_loop.py`).
/// This runs automatically when goose is not installed.
async fn run_with_python(app_handle: AppHandle, idea: String) -> Result<String, String> {
    let python_cmd = if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
    };

    let mut child = Command::new(python_cmd)
        .args([
            "python_agent/agent_loop.py",
            "--idea",
            &idea,
            "--force-python",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start Python agent: {e}"))?;

    stream_to_frontend(&app_handle, &mut child);

    match child.wait() {
        Ok(s) if s.success() => Ok("Python agent completed successfully".to_string()),
        Ok(_) => Err("Python agent exited with a non-zero status".to_string()),
        Err(e) => Err(format!("Python agent failed: {e}")),
    }
}

/// Tauri command exposed to the frontend.
///
/// Tries **goose** first (the recommended engine).  If goose is not installed,
/// automatically falls back to the Python agent so the app remains functional
/// out of the box.
#[tauri::command]
pub async fn run_agent(app_handle: AppHandle, idea: String) -> Result<String, String> {
    match find_goose() {
        Some(goose_bin) => run_with_goose(app_handle, idea, goose_bin).await,
        None => {
            app_handle
                .emit(
                    "agent-log",
                    "[AGENT_THINKING] goose not found — using Python fallback agent. \
                     Install goose: curl -fsSL \
                     https://github.com/aaif-goose/goose/releases/download/stable/download_cli.sh \
                     | bash",
                )
                .unwrap_or_default();
            run_with_python(app_handle, idea).await
        }
    }
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

    /// Serialise tests that mutate the process-global working directory.
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    // ── OrIfEmpty ────────────────────────────────────────────────────────────

    #[test]
    fn test_or_if_empty_uses_self_when_non_empty() {
        assert_eq!(
            "hello".to_string().or_if_empty("fallback".to_string()),
            "hello"
        );
    }

    #[test]
    fn test_or_if_empty_uses_fallback_when_empty() {
        assert_eq!(
            "".to_string().or_if_empty("fallback".to_string()),
            "fallback"
        );
    }

    #[test]
    fn test_or_if_empty_fallback_also_empty() {
        assert_eq!("".to_string().or_if_empty("".to_string()), "");
    }

    // ── first_env ────────────────────────────────────────────────────────────

    #[test]
    fn test_first_env_returns_none_when_no_vars_set() {
        // Use an unlikely-to-be-set variable name.
        std::env::remove_var("_TEST_RUSTTERM_NOEXIST_1");
        std::env::remove_var("_TEST_RUSTTERM_NOEXIST_2");
        let result = first_env(&["_TEST_RUSTTERM_NOEXIST_1", "_TEST_RUSTTERM_NOEXIST_2"]);
        assert!(result.is_none());
    }

    #[test]
    fn test_first_env_returns_first_set_var() {
        std::env::set_var("_TEST_RUSTTERM_A", "value_a");
        std::env::set_var("_TEST_RUSTTERM_B", "value_b");

        let result = first_env(&["_TEST_RUSTTERM_A", "_TEST_RUSTTERM_B"]);

        std::env::remove_var("_TEST_RUSTTERM_A");
        std::env::remove_var("_TEST_RUSTTERM_B");

        assert_eq!(result, Some("value_a".to_string()));
    }

    #[test]
    fn test_first_env_skips_empty_value() {
        std::env::set_var("_TEST_RUSTTERM_EMPTY", "");
        std::env::set_var("_TEST_RUSTTERM_NONEMPTY", "ok");

        let result = first_env(&["_TEST_RUSTTERM_EMPTY", "_TEST_RUSTTERM_NONEMPTY"]);

        std::env::remove_var("_TEST_RUSTTERM_EMPTY");
        std::env::remove_var("_TEST_RUSTTERM_NONEMPTY");

        assert_eq!(result, Some("ok".to_string()));
    }

    // ── load_app_config ──────────────────────────────────────────────────────

    #[test]
    fn test_load_app_config_returns_default_when_file_missing() {
        let _lock = CWD_LOCK.lock().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let cfg = load_app_config();

        std::env::set_current_dir(original).unwrap();

        assert!(cfg.llm.provider.is_empty());
        assert!(cfg.llm.api_key.is_empty());
    }

    #[test]
    fn test_load_app_config_reads_provider_and_key() {
        let _lock = CWD_LOCK.lock().unwrap();
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "llm:\n  provider: kimi\n  api_key: test-key-123").unwrap();

        let tmp_dir = f.path().parent().unwrap().to_path_buf();
        let config_path = tmp_dir.join("config.yaml");
        std::fs::copy(f.path(), &config_path).unwrap();

        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp_dir).unwrap();

        let cfg = load_app_config();

        std::env::set_current_dir(original).unwrap();
        std::fs::remove_file(config_path).unwrap();

        assert_eq!(cfg.llm.provider, "kimi");
        assert_eq!(cfg.llm.api_key, "test-key-123");
    }

    // ── llm_env_for_goose ────────────────────────────────────────────────────

    #[test]
    fn test_llm_env_for_goose_openai_default() {
        let _lock = CWD_LOCK.lock().unwrap();
        // Ensure no stray env vars interfere.
        for v in &[
            "LLM_PROVIDER",
            "LLM_API_KEY",
            "LLM_MODEL",
            "LLM_BASE_URL",
            "OPENAI_API_KEY",
            "OPENAI_BASE_URL",
            "GOOSE_MODEL",
            "GOOSE_PROVIDER",
        ] {
            std::env::remove_var(v);
        }

        let tmp = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let env = llm_env_for_goose();

        std::env::set_current_dir(original).unwrap();

        let provider = env
            .iter()
            .find(|(k, _)| k == "GOOSE_PROVIDER")
            .map(|(_, v)| v.as_str());
        assert_eq!(provider, Some("openai"));

        let model = env
            .iter()
            .find(|(k, _)| k == "GOOSE_MODEL")
            .map(|(_, v)| v.as_str());
        assert_eq!(model, Some("gpt-4o-mini"));
    }

    #[test]
    fn test_llm_env_for_goose_kimi_provider() {
        let _lock = CWD_LOCK.lock().unwrap();
        std::env::set_var("LLM_PROVIDER", "kimi");
        std::env::set_var("KIMI_API_KEY", "kimi-key-xyz");
        for v in &[
            "LLM_API_KEY",
            "OPENAI_API_KEY",
            "LLM_MODEL",
            "LLM_BASE_URL",
            "OPENAI_BASE_URL",
            "GOOSE_PROVIDER",
            "GOOSE_MODEL",
        ] {
            std::env::remove_var(v);
        }

        let tmp = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let env = llm_env_for_goose();

        std::env::set_current_dir(original).unwrap();
        std::env::remove_var("LLM_PROVIDER");
        std::env::remove_var("KIMI_API_KEY");

        let base = env
            .iter()
            .find(|(k, _)| k == "OPENAI_BASE_URL")
            .map(|(_, v)| v.as_str());
        assert_eq!(base, Some("https://api.moonshot.cn/v1"));

        let key = env
            .iter()
            .find(|(k, _)| k == "OPENAI_API_KEY")
            .map(|(_, v)| v.as_str());
        assert_eq!(key, Some("kimi-key-xyz"));

        let model = env
            .iter()
            .find(|(k, _)| k == "GOOSE_MODEL")
            .map(|(_, v)| v.as_str());
        assert_eq!(model, Some("moonshot-v1-8k"));
    }

    #[test]
    fn test_llm_env_for_goose_azure_provider() {
        let _lock = CWD_LOCK.lock().unwrap();
        std::env::set_var("LLM_PROVIDER", "azure");
        std::env::set_var("AZURE_OPENAI_API_KEY", "az-key");
        std::env::set_var("AZURE_OPENAI_ENDPOINT", "https://my.openai.azure.com/");
        for v in &[
            "LLM_API_KEY",
            "OPENAI_API_KEY",
            "LLM_MODEL",
            "LLM_BASE_URL",
            "GOOSE_PROVIDER",
            "GOOSE_MODEL",
            "AZURE_OPENAI_DEPLOYMENT",
            "AZURE_OPENAI_API_VERSION",
        ] {
            std::env::remove_var(v);
        }

        let tmp = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();

        let env = llm_env_for_goose();

        std::env::set_current_dir(original).unwrap();
        std::env::remove_var("LLM_PROVIDER");
        std::env::remove_var("AZURE_OPENAI_API_KEY");
        std::env::remove_var("AZURE_OPENAI_ENDPOINT");

        let provider = env
            .iter()
            .find(|(k, _)| k == "GOOSE_PROVIDER")
            .map(|(_, v)| v.as_str());
        assert_eq!(provider, Some("azure"));

        // Azure path should NOT include OPENAI_BASE_URL
        assert!(!env.iter().any(|(k, _)| k == "OPENAI_BASE_URL"));
    }

    // ── find_goose ───────────────────────────────────────────────────────────

    #[test]
    fn test_find_goose_returns_option() {
        // Just verify the function runs without panicking.  The return value
        // depends on whether goose is installed in the CI environment.
        let _result: Option<String> = find_goose();
    }

    // ── AzureConfig default ──────────────────────────────────────────────────

    #[test]
    fn test_azure_config_default_is_empty() {
        let cfg = AzureConfig::default();
        assert!(cfg.endpoint.is_empty());
        assert!(cfg.deployment.is_empty());
        assert!(cfg.api_version.is_empty());
    }
}
