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

    // Helper: env var → config.yaml fallback → empty string
    let resolve = |env_var: &str, yaml_val: &str| -> String {
        std::env::var(env_var)
            .unwrap_or_default()
            .trim()
            .to_string()
            .or_if_empty(yaml_val.trim().to_string())
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

