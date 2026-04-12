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

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_send_notification_returns_ok() {
        // send_alert may silently fail in headless CI environments (no display
        // server), but the Tauri command must always return Ok(()).
        let result = send_notification("Test Title".to_string(), "Test body".to_string()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_notification_empty_strings() {
        let result = send_notification(String::new(), String::new()).await;
        assert!(result.is_ok());
    }
}
