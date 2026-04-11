use tauri::Manager;

mod agent_runner;
mod market_engine;
mod notifier;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            // Automatically start the market engine on launch
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = market_engine::start_market_engine(handle).await {
                    eprintln!("[lib] Market engine failed to start: {}", e);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            agent_runner::run_agent,
            market_engine::start_market_engine,
            market_engine::get_strategies,
            market_engine::toggle_strategy,
            market_engine::get_poll_interval,
            market_engine::set_poll_interval,
            notifier::send_notification,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
