// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod agent_runner;
mod market_engine;
mod notifier;

fn main() {
    ai_quant_terminal_lib::run()
}
