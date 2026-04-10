use tauri::{AppHandle, Emitter};
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};

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

