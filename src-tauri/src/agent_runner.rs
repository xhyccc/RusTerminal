use tauri::{AppHandle, Emitter};
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader};

#[tauri::command]
pub async fn run_agent(app_handle: AppHandle, idea: String) -> Result<String, String> {
    let python_cmd = if cfg!(target_os = "windows") { "python" } else { "python3" };

    let mut child = Command::new(python_cmd)
        .arg("python_agent/agent_loop.py")
        .arg("--idea")
        .arg(&idea)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to start agent: {}", e))?;

    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);

    for line in reader.lines() {
        if let Ok(line) = line {
            app_handle.emit("agent-log", line).unwrap_or_default();
        }
    }

    let status = child.wait().map_err(|e| format!("Agent failed: {}", e))?;

    if status.success() {
        Ok("Agent completed successfully".to_string())
    } else {
        Err("Agent failed".to_string())
    }
}
