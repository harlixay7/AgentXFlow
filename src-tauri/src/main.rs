// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--daemon" || a == "--headless" || a == "-d") {
        let rt = tokio::runtime::Runtime::new().expect("Failed to build tokio runtime");
        if let Err(e) = rt.block_on(agent_x_flow_lib::run_daemon()) {
            eprintln!("Daemon error: {}", e);
            std::process::exit(1);
        }
    } else {
        agent_x_flow_lib::run()
    }
}
