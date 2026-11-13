//! The named-pipe half of the Hangar SDK (protocol v1): Marquee serves
//! `\\.\pipe\larosa023.marquee.<session>`, announces itself in
//! `%LOCALAPPDATA%\Hangar\run\marquee.json`, and sends `health` / `busy` / `idle` / `restored`
//! messages to Hangar's own pipe. What each request means lives in `marquee_core::hangar`.

use marquee_core::hangar;
#[cfg(windows)]
use marquee_core::hangar::After;
use marquee_core::Ctx;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tauri::AppHandle;

/// What the window wants back after a live update (route, search, scroll). The UI keeps it current.
#[derive(Default)]
pub struct UiState(pub Mutex<Value>);

#[cfg(windows)]
fn session_id() -> u32 {
    let mut session = 0u32;
    unsafe {
        windows_sys::Win32::System::RemoteDesktop::ProcessIdToSessionId(windows_sys::Win32::System::Threading::GetCurrentProcessId(), &mut session);
    }
    session
}

#[cfg(windows)]
pub fn start(app: AppHandle, ctx: Arc<Ctx>, ui: Arc<UiState>) {
    use tauri::Manager;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ServerOptions;

    let Some(local) = crate::system::local_app_data() else { return };
    // Only speak the protocol when Hangar is on this PC.
    if !hangar::installed(&local) {
        return;
    }
    let session = session_id();
    let pipe = hangar::pipe_name(session);
    let run_file = hangar::run_file(&local);
    if let Some(dir) = run_file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let exe = std::env::current_exe().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
    let _ = std::fs::write(&run_file, hangar::run_file_contents(std::process::id(), session, &ctx.version, &exe).to_string());

    tauri::async_runtime::spawn(async move {
        loop {
            // One client at a time, local connections only.
            let server = match ServerOptions::new().reject_remote_clients(true).max_instances(1).create(&pipe) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("Hangar pipe couldn't be created: {e}");
                    return;
                }
            };
            if server.connect().await.is_err() {
                continue;
            }
            let (read, mut write) = tokio::io::split(server);
            let mut lines = BufReader::new(read).lines();
            let mut exit = false;
            while let Ok(Some(line)) = lines.next_line().await {
                if line.len() > 1024 * 1024 {
                    break;
                }
                let Ok(req) = serde_json::from_str::<Value>(&line) else { continue };
                let state = ui.0.lock().map(|s| s.clone()).unwrap_or(Value::Null);
                let ctx2 = ctx.clone();
                // Backups and restores touch the disk: keep them off the async threads.
                let (reply, after) = match tokio::task::spawn_blocking(move || hangar::handle(&ctx2, &req, &state)).await {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                match after {
                    After::Exit => {
                        // `restart` gets no reply: the pipe closing is the answer.
                        exit = true;
                        break;
                    }
                    After::CheckUpdate => crate::updates::check_in_background(app.clone(), true),
                    After::Nothing => {}
                }
                let mut out = reply.to_string();
                out.push('\n');
                if write.write_all(out.as_bytes()).await.is_err() {
                    break;
                }
                let _ = write.flush().await;
            }
            if exit {
                let _ = std::fs::remove_file(hangar::run_file(&local));
                app.state::<crate::Quitting>().0.store(true, std::sync::atomic::Ordering::SeqCst);
                app.exit(0);
                return;
            }
        }
    });
}

#[cfg(not(windows))]
pub fn start(_app: AppHandle, _ctx: Arc<Ctx>, _ui: Arc<UiState>) {}

/// Sends one message to Hangar, if Hangar is running. Never blocks the app.
pub fn send(message: Value) {
    #[cfg(windows)]
    tauri::async_runtime::spawn(async move {
        use tokio::io::AsyncWriteExt;
        use tokio::net::windows::named_pipe::ClientOptions;
        let Some(local) = crate::system::local_app_data() else { return };
        if !hangar::installed(&local) {
            return;
        }
        let pipe = hangar::hangar_pipe_name(session_id());
        for _ in 0..3 {
            match ClientOptions::new().open(&pipe) {
                Ok(mut client) => {
                    let mut line = message.to_string();
                    line.push('\n');
                    let _ = client.write_all(line.as_bytes()).await;
                    let _ = client.flush().await;
                    return;
                }
                // The pipe exists but is busy with another app: wait a moment and try again.
                Err(e) if e.raw_os_error() == Some(231) => tokio::time::sleep(std::time::Duration::from_millis(400)).await,
                Err(_) => return, // Hangar isn't running: nothing to tell.
            }
        }
    });
    #[cfg(not(windows))]
    let _ = message;
}

pub fn cleanup() {
    if let Some(local) = crate::system::local_app_data() {
        let _ = std::fs::remove_file(hangar::run_file(&local));
    }
}
