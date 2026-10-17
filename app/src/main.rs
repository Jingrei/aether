#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod engine_host;
mod offline;
mod platform;
mod server;
mod state;
mod tray;

use std::sync::atomic::Ordering;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = |f: &dyn Fn() -> anyhow::Result<()>| {
        platform::attach_console();
        if let Err(e) = f() {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    match args.first().map(String::as_str) {
        Some("render") => return cli(&|| offline::render(&args[1..])),
        Some("devices") => return cli(&|| Ok(offline::list_devices())),
        Some("profiles") => return cli(&|| Ok(offline::list_profiles())),
        Some("--help" | "-h" | "help") => return cli(&|| Ok(print!("{}", offline::USAGE))),
        _ => {}
    }
    let background = args.iter().any(|a| a == "--background");
    let url = format!("http://127.0.0.1:{}/", server::PORT);

    // Single instance: if the port is taken, Aether is already running, so just show its window.
    let listener = match std::net::TcpListener::bind(("127.0.0.1", server::PORT)) {
        Ok(l) => l,
        Err(_) => {
            platform::open_ui(&url);
            return;
        }
    };

    let app = state::App::load();
    app.attach_audio(audio::spawn(app.clone()));

    let server_app = app.clone();
    std::thread::Builder::new()
        .name("aether-server".into())
        .spawn(move || server::serve(listener, server_app))
        .expect("spawn server");

    if !background {
        platform::open_ui(&url);
    }
    if let Err(e) = tray::run(app.clone(), url) {
        eprintln!("tray error: {e}");
        // Without a tray we still keep running until asked to quit via the UI.
        while !app.quit.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
    app.shutdown();
    std::process::exit(0);
}
