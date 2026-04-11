use tauri::{AppHandle, Emitter};
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};

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
            ("AZURE_OPENAI_API_KEY",    cfg.llm.api_key.as_str()),
            ("AZURE_OPENAI_ENDPOINT",   cfg.llm.azure.endpoint.as_str()),
            ("AZURE_OPENAI_DEPLOYMENT", cfg.llm.azure.deployment.as_str()),
            ("AZURE_OPENAI_API_VERSION",cfg.llm.azure.api_version.as_str()),
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
    let model = resolve("LLM_MODEL", &cfg.llm.model)
        .or_if_empty(default_model.to_string());
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
        if self.is_empty() { fallback } else { self }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    // -- OrIfEmpty -----------------------------------------------------------

    #[test]
    fn or_if_empty_returns_self_when_non_empty() {
        let s = "hello".to_string();
        assert_eq!(s.or_if_empty("fallback".to_string()), "hello");
    }

    #[test]
    fn or_if_empty_returns_fallback_when_empty() {
        let s = String::new();
        assert_eq!(s.or_if_empty("fallback".to_string()), "fallback");
    }

    #[test]
    fn or_if_empty_with_empty_fallback() {
        let s = String::new();
        assert_eq!(s.or_if_empty(String::new()), "");
    }

    // -- first_env -----------------------------------------------------------

    #[test]
    fn first_env_returns_none_when_all_unset() {
        assert!(first_env(&["__TEST_UNSET_VAR_A1__", "__TEST_UNSET_VAR_A2__"]).is_none());
    }

    #[test]
    fn first_env_returns_none_for_empty_slice() {
        assert!(first_env(&[]).is_none());
    }

    #[test]
    fn first_env_returns_first_non_empty_var() {
        let key = "__FIRST_ENV_TEST_NON_EMPTY__";
        env::set_var(key, "test_value");
        let result = first_env(&["__UNSET_BEFORE__", key]);
        env::remove_var(key);
        assert_eq!(result.as_deref(), Some("test_value"));
    }

    #[test]
    fn first_env_skips_empty_string_values() {
        let empty_key = "__FIRST_ENV_EMPTY_STR__";
        let val_key = "__FIRST_ENV_FOUND_STR__";
        env::set_var(empty_key, "");
        env::set_var(val_key, "found_it");
        let result = first_env(&[empty_key, val_key]);
        env::remove_var(empty_key);
        env::remove_var(val_key);
        assert_eq!(result.as_deref(), Some("found_it"));
    }

    #[test]
    fn first_env_returns_first_when_multiple_set() {
        let key1 = "__FIRST_ENV_MULTI_1__";
        let key2 = "__FIRST_ENV_MULTI_2__";
        env::set_var(key1, "first");
        env::set_var(key2, "second");
        let result = first_env(&[key1, key2]);
        env::remove_var(key1);
        env::remove_var(key2);
        assert_eq!(result.as_deref(), Some("first"));
    }

    // -- load_app_config -----------------------------------------------------

    #[test]
    fn load_app_config_returns_default_when_no_file() {
        // Tests run from src-tauri/; there is no config.yaml there.
        let cfg = load_app_config();
        assert_eq!(cfg.llm.provider, "");
        assert_eq!(cfg.llm.api_key, "");
        assert_eq!(cfg.llm.model, "");
        assert_eq!(cfg.llm.base_url, "");
    }

    // -- llm_env_for_goose ---------------------------------------------------

    /// Remove all LLM-related env vars so tests start from a clean slate.
    fn clear_llm_env_vars() {
        for v in &[
            "LLM_PROVIDER",
            "LLM_API_KEY",
            "LLM_MODEL",
            "LLM_BASE_URL",
            "OPENAI_API_KEY",
            "KIMI_API_KEY",
            "GLM_API_KEY",
            "SILICONFLOW_API_KEY",
            "AZURE_OPENAI_API_KEY",
            "AZURE_OPENAI_ENDPOINT",
            "AZURE_OPENAI_DEPLOYMENT",
            "AZURE_OPENAI_API_VERSION",
        ] {
            env::remove_var(v);
        }
    }

    fn env_map(pairs: &[(String, String)]) -> std::collections::HashMap<String, String> {
        pairs.iter().cloned().collect()
    }

    #[test]
    fn llm_env_default_provider_is_openai_when_no_provider_set() {
        clear_llm_env_vars();
        let vars = env_map(&llm_env_for_goose());
        assert_eq!(vars.get("GOOSE_PROVIDER").map(String::as_str), Some("openai"));
    }

    #[test]
    fn llm_env_openai_sets_default_model() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "openai");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        assert_eq!(vars.get("GOOSE_MODEL").map(String::as_str), Some("gpt-4o-mini"));
        assert_eq!(vars.get("GOOSE_PROVIDER").map(String::as_str), Some("openai"));
    }

    #[test]
    fn llm_env_kimi_sets_base_url_and_default_model() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "kimi");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        assert_eq!(
            vars.get("OPENAI_BASE_URL").map(String::as_str),
            Some("https://api.moonshot.cn/v1")
        );
        assert_eq!(
            vars.get("GOOSE_MODEL").map(String::as_str),
            Some("moonshot-v1-8k")
        );
    }

    #[test]
    fn llm_env_glm_sets_base_url_and_default_model() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "glm");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        assert_eq!(
            vars.get("OPENAI_BASE_URL").map(String::as_str),
            Some("https://open.bigmodel.cn/api/paas/v4")
        );
        assert_eq!(
            vars.get("GOOSE_MODEL").map(String::as_str),
            Some("glm-4-flash")
        );
    }

    #[test]
    fn llm_env_siliconflow_sets_base_url_and_default_model() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "siliconflow");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        assert_eq!(
            vars.get("OPENAI_BASE_URL").map(String::as_str),
            Some("https://api.siliconflow.cn/v1")
        );
        assert_eq!(
            vars.get("GOOSE_MODEL").map(String::as_str),
            Some("Qwen/Qwen2.5-7B-Instruct")
        );
    }

    #[test]
    fn llm_env_azure_sets_goose_provider_to_azure() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "azure");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        assert_eq!(vars.get("GOOSE_PROVIDER").map(String::as_str), Some("azure"));
        // Azure path returns early and must NOT set GOOSE_MODEL
        assert!(
            vars.get("GOOSE_MODEL").is_none(),
            "Azure path should not set GOOSE_MODEL"
        );
    }

    #[test]
    fn llm_env_azure_injects_azure_vars_from_env() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "azure");
        env::set_var("AZURE_OPENAI_API_KEY", "az-key");
        env::set_var("AZURE_OPENAI_ENDPOINT", "https://my.openai.azure.com");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        env::remove_var("AZURE_OPENAI_API_KEY");
        env::remove_var("AZURE_OPENAI_ENDPOINT");
        assert_eq!(
            vars.get("AZURE_OPENAI_API_KEY").map(String::as_str),
            Some("az-key")
        );
        assert_eq!(
            vars.get("AZURE_OPENAI_ENDPOINT").map(String::as_str),
            Some("https://my.openai.azure.com")
        );
    }

    #[test]
    fn llm_env_api_key_forwarded_for_openai() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "openai");
        env::set_var("OPENAI_API_KEY", "sk-test-key");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        env::remove_var("OPENAI_API_KEY");
        assert_eq!(
            vars.get("OPENAI_API_KEY").map(String::as_str),
            Some("sk-test-key")
        );
    }

    #[test]
    fn llm_env_no_api_key_omitted_from_result() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "openai");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        assert!(
            vars.get("OPENAI_API_KEY").is_none(),
            "OPENAI_API_KEY should not appear when not set"
        );
    }

    #[test]
    fn llm_env_model_override_from_llm_model_env_var() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "openai");
        env::set_var("LLM_MODEL", "gpt-4");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        env::remove_var("LLM_MODEL");
        assert_eq!(vars.get("GOOSE_MODEL").map(String::as_str), Some("gpt-4"));
    }

    #[test]
    fn llm_env_base_url_override_from_env_var() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "openai");
        env::set_var("LLM_BASE_URL", "https://custom.openai.example.com/v1");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        env::remove_var("LLM_BASE_URL");
        assert_eq!(
            vars.get("OPENAI_BASE_URL").map(String::as_str),
            Some("https://custom.openai.example.com/v1")
        );
    }

    #[test]
    fn llm_env_kimi_uses_kimi_api_key_first() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "kimi");
        env::set_var("KIMI_API_KEY", "kimi-secret");
        env::set_var("OPENAI_API_KEY", "should-not-be-used");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        env::remove_var("KIMI_API_KEY");
        env::remove_var("OPENAI_API_KEY");
        assert_eq!(
            vars.get("OPENAI_API_KEY").map(String::as_str),
            Some("kimi-secret")
        );
    }

    #[test]
    fn llm_env_glm_uses_glm_api_key_first() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "glm");
        env::set_var("GLM_API_KEY", "glm-secret");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        env::remove_var("GLM_API_KEY");
        assert_eq!(
            vars.get("OPENAI_API_KEY").map(String::as_str),
            Some("glm-secret")
        );
    }

    #[test]
    fn llm_env_siliconflow_uses_siliconflow_api_key_first() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "siliconflow");
        env::set_var("SILICONFLOW_API_KEY", "sf-secret");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        env::remove_var("SILICONFLOW_API_KEY");
        assert_eq!(
            vars.get("OPENAI_API_KEY").map(String::as_str),
            Some("sf-secret")
        );
    }

    #[test]
    fn llm_env_unknown_provider_defaults_to_openai() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "nonexistent_provider");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        assert_eq!(vars.get("GOOSE_PROVIDER").map(String::as_str), Some("openai"));
        assert_eq!(vars.get("GOOSE_MODEL").map(String::as_str), Some("gpt-4o-mini"));
    }

    #[test]
    fn llm_env_provider_matching_is_case_insensitive() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "KIMI");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        assert_eq!(
            vars.get("OPENAI_BASE_URL").map(String::as_str),
            Some("https://api.moonshot.cn/v1"),
            "Provider matching should be case-insensitive"
        );
    }

    #[test]
    fn llm_env_openai_does_not_set_base_url_by_default() {
        clear_llm_env_vars();
        env::set_var("LLM_PROVIDER", "openai");
        let vars = env_map(&llm_env_for_goose());
        env::remove_var("LLM_PROVIDER");
        assert!(
            vars.get("OPENAI_BASE_URL").is_none(),
            "OpenAI provider should not set OPENAI_BASE_URL unless explicitly configured"
        );
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
        .args(["run", "--recipe", recipe, "--params", &params, "--no-session"])
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
        .args(["python_agent/agent_loop.py", "--idea", &idea, "--force-python"])
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

