use notify_rust::Notification;

/// Send a desktop notification with given title and body.
pub fn send_alert(title: &str, body: &str) {
    if let Err(e) = Notification::new()
        .summary(title)
        .body(body)
        .icon("dialog-information")
        .show()
    {
        eprintln!("[notifier] Failed to send notification: {}", e);
    }
}

#[tauri::command]
pub async fn send_notification(title: String, body: String) -> Result<(), String> {
    send_alert(&title, &body);
    Ok(())
}
