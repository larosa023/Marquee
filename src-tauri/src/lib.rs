//! The Marquee app shell: window, tray, toasts, updater, Hangar pipe and background scheduling.
//! All the real work happens in `marquee-core`; this crate wires it to Windows and the UI.

mod hangar_pipe;
mod system;
mod tray;
mod updates;

use marquee_core::ctx::Host;
use marquee_core::error::ErrorPayload;
use marquee_core::net::{Http, ReqwestTransport};
use marquee_core::{api, hangar, notifications, store, updater, Ctx};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

/// Set when the user (or an update, or Hangar) really wants the app to exit rather than hide.
pub struct Quitting(pub AtomicBool);
/// A route for a window that is still being created to open on.
pub struct PendingRoute(pub Mutex<Option<String>>);
/// How this launch started.
struct Launch {
    hidden: bool,
    restore_state: Mutex<Option<Value>>,
    portable: bool,
    default_data_dir: std::path::PathBuf,
    crashed: Mutex<Option<String>>,
    _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

struct TauriHost(AppHandle);

impl Host for TauriHost {
    fn open_url(&self, url: &str) {
        if let Err(e) = self.0.opener().open_url(url, None::<&str>) {
            tracing::warn!("couldn't open {url}: {e}");
        }
    }
    fn emit(&self, event: &str, payload: Value) {
        let _ = self.0.emit(event, payload);
        if event == "busy:changed" {
            let reason = payload_reason(&self.0);
            hangar_pipe::send(hangar::busy_message(reason.is_some(), reason.as_deref().unwrap_or("")));
        }
    }
}

fn payload_reason(app: &AppHandle) -> Option<String> {
    app.try_state::<Arc<Ctx>>().and_then(|c| c.busy_reason())
}

pub fn quit(app: &AppHandle) {
    app.state::<Quitting>().0.store(true, Ordering::SeqCst);
    hangar_pipe::cleanup();
    app.exit(0);
}

pub fn sync_now(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let ctx = app.state::<Arc<Ctx>>().inner().clone();
        if let Err(e) = api::run_sync(&ctx, None).await {
            tracing::warn!("sync failed: {e}");
        }
        show_toasts(&app);
    });
}

/// Shows any notifications that are due as Windows toasts (none during quiet hours).
fn show_toasts(app: &AppHandle) {
    let ctx = app.state::<Arc<Ctx>>().inner().clone();
    match notifications::toastable(&ctx.db) {
        Ok(list) => {
            for n in list {
                if let Err(e) = app.notification().builder().title(&n.title).body(&n.body).show() {
                    tracing::warn!("toast failed: {e}");
                }
            }
        }
        Err(e) => tracing::warn!("couldn't read notifications: {e}"),
    }
}

/// The one command the UI uses to reach the engine.
#[tauri::command]
async fn call(app: AppHandle, method: String, params: Option<Value>) -> Result<Value, Value> {
    let ctx = app.state::<Arc<Ctx>>().inner().clone();
    let result = api::dispatch(&ctx, &method, params.unwrap_or(Value::Null)).await;
    if method == "sync" {
        show_toasts(&app);
    }
    result.map_err(|e| {
        tracing::debug!("{method} failed: {e}");
        serde_json::to_value(ErrorPayload::from(&e)).unwrap_or(Value::Null)
    })
}

/// Called by the UI once it has painted, so the window never flashes white or empty.
#[tauri::command]
fn app_ready(app: AppHandle, window: tauri::WebviewWindow) -> Value {
    let launch = app.state::<Launch>();
    if window.label() == "main" && !launch.hidden {
        let _ = window.show();
        let _ = window.set_focus();
    }
    let route = app.state::<PendingRoute>().0.lock().unwrap().take();
    json!({ "route": route, "restore_state": launch.restore_state.lock().unwrap().take() })
}

#[tauri::command]
fn system_info(app: AppHandle) -> Value {
    let ctx = app.state::<Arc<Ctx>>().inner().clone();
    let launch = app.state::<Launch>();
    json!({
        "version": ctx.version,
        "accent": system::accent_color(),
        "portable": launch.portable,
        "data_dir": ctx.data_dir.to_string_lossy(),
        "default_data_dir": launch.default_data_dir.to_string_lossy(),
        "log_dir": ctx.data_dir.join("logs").to_string_lossy(),
        "hangar_installed": updates::hangar_installed(),
        "deferred_to_hangar": updates::deferred_to_hangar(&ctx),
        "crashed": launch.crashed.lock().unwrap().clone(),
        "platform": std::env::consts::OS,
    })
}

#[tauri::command]
fn clear_crash(app: AppHandle) {
    let ctx = app.state::<Arc<Ctx>>().inner().clone();
    let _ = std::fs::remove_file(ctx.data_dir.join("crash.txt"));
    *app.state::<Launch>().crashed.lock().unwrap() = None;
}

/// "mica" | "acrylic" | "none": the window backdrop the current theme asks for.
#[tauri::command]
fn set_backdrop(window: tauri::WebviewWindow, kind: String) {
    use tauri::window::{Effect, EffectsBuilder};
    let effects = match kind.as_str() {
        "mica" => Some(EffectsBuilder::new().effect(Effect::Mica).build()),
        "acrylic" => Some(EffectsBuilder::new().effect(Effect::Acrylic).build()),
        _ => None,
    };
    let _ = window.set_effects(effects);
}

/// The UI reports where it is so a live update can put the user back in the same place.
#[tauri::command]
fn set_ui_state(app: AppHandle, state: Value) {
    if let Ok(mut s) = app.state::<Arc<hangar_pipe::UiState>>().0.lock() {
        *s = state;
    }
}

#[tauri::command]
fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    // Web pages, plus the two app links Marquee uses on purpose.
    let ok = ["https://", "http://", "steam://", "spotify:"].iter().any(|p| url.starts_with(p));
    if !ok {
        return Err("Marquee only opens web, Steam and Spotify links.".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_folder(app: AppHandle, which: String) -> Result<(), String> {
    let ctx = app.state::<Arc<Ctx>>().inner().clone();
    let dir = match which.as_str() {
        "logs" => ctx.data_dir.join("logs"),
        "themes" => ctx.data_dir.join("themes"),
        "backups" => ctx.data_dir.join("backups"),
        _ => ctx.data_dir.clone(),
    };
    let _ = std::fs::create_dir_all(&dir);
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|e| e.to_string())
}

/// Writes a file the user just chose in a save dialog (a shared list image or its Markdown).
#[tauri::command]
fn save_file(path: String, contents: String, base64: bool) -> Result<(), String> {
    use base64::Engine;
    let bytes = if base64 { base64::engine::general_purpose::STANDARD.decode(contents.as_bytes()).map_err(|_| "That image couldn't be saved.".to_string())? } else { contents.into_bytes() };
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("That file is too large to save.".into());
    }
    std::fs::write(&path, bytes).map_err(|e| format!("Couldn't save to {path}: {e}"))
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    quit(&app);
}

/// Moves the data folder and restarts into it.
#[tauri::command]
fn set_data_location(app: AppHandle, path: String) -> Result<(), String> {
    let ctx = app.state::<Arc<Ctx>>().inner().clone();
    let launch = app.state::<Launch>();
    if launch.portable {
        return Err("In portable mode the data stays in the data folder next to Marquee.exe.".into());
    }
    ctx.db.checkpoint().map_err(|e| e.to_string())?;
    system::move_data(&ctx.data_dir, &launch.default_data_dir, std::path::Path::new(&path)).map_err(|e| e.to_string())?;
    app.state::<Quitting>().0.store(true, Ordering::SeqCst);
    app.restart();
}

/// Serves cached artwork to the UI as `mqimg://localhost/?u=<url>`.
fn serve_image(app: AppHandle, request: tauri::http::Request<Vec<u8>>, responder: tauri::UriSchemeResponder) {
    use tauri::http::Response;
    tauri::async_runtime::spawn(async move {
        let url = request.uri().query().and_then(|q| q.split('&').find_map(|p| p.strip_prefix("u="))).and_then(|u| urlencoding::decode(u).ok()).map(|u| u.into_owned());
        let ctx = app.try_state::<Arc<Ctx>>().map(|c| c.inner().clone());
        let response = match (url, ctx) {
            (Some(url), Some(ctx)) => match marquee_core::images::get(&ctx, &url).await {
                Ok((bytes, mime)) => Response::builder().status(200).header("Content-Type", mime).header("Cache-Control", "public, max-age=86400").header("Access-Control-Allow-Origin", "*").body(bytes),
                Err(_) => Response::builder().status(404).body(Vec::new()),
            },
            _ => Response::builder().status(400).body(Vec::new()),
        };
        if let Ok(r) = response {
            responder.respond(r);
        }
    });
}

/// Handles command-line requests that don't need a window. Returns true if the app should exit.
fn run_cli(args: &[String], ctx: &Arc<Ctx>) -> bool {
    let value_of = |flag: &str| args.windows(2).find(|w| w[0] == flag).map(|w| w[1].clone());
    if let Some(file) = value_of("--backup") {
        match marquee_core::backup::create(ctx, Some(std::path::Path::new(&file)), false) {
            Ok(p) => println!("{}", p.display()),
            Err(e) => eprintln!("{e}"),
        }
        return true;
    }
    if let Some(file) = value_of("--restore") {
        match marquee_core::backup::restore(ctx, std::path::Path::new(&file)) {
            Ok(_) => println!("restored"),
            Err(e) => eprintln!("{e}"),
        }
        return true;
    }
    false
}

/// Background work: sync on launch and on the chosen interval, update checks, queued toasts,
/// the health signals, and letting the window go when it has sat in the tray for a while.
fn start_background(app: AppHandle, ctx: Arc<Ctx>) {
    // Healthy = still running after a few seconds. Tells the rollback logic and Hangar.
    {
        let (app, ctx) = (app.clone(), ctx.clone());
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(2)).await;
            hangar_pipe::send(hangar::health_message(&ctx.version, std::process::id()));
            tokio::time::sleep(Duration::from_secs(6)).await;
            updater::mark_healthy(&ctx.data_dir, &ctx.version);
            updates::check_in_background(app, false);
        });
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let onboarded = || store::setting_or(&ctx.db, "onboarded", false);
        if onboarded() && store::setting_or(&ctx.db, "sync.on_launch", true) {
            let _ = api::run_sync(&ctx, None).await;
            show_toasts(&app);
        }
        let mut last_update_check = std::time::Instant::now();
        let mut hidden_for = 0u32;
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            show_toasts(&app);

            let hours: f64 = store::setting_or(&ctx.db, "sync.interval_hours", 6.0);
            let last: Option<Value> = store::get_setting(&ctx.db, "last_sync");
            let due = last.as_ref().and_then(|l| l["at"].as_str()).and_then(chrono_since).map(|mins| hours > 0.0 && mins >= hours * 60.0).unwrap_or(true);
            if onboarded() && due && ctx.busy_reason().is_none() {
                let _ = api::run_sync(&ctx, None).await;
                show_toasts(&app);
            }
            if last_update_check.elapsed() >= Duration::from_secs(6 * 3600) {
                last_update_check = std::time::Instant::now();
                updates::check_in_background(app.clone(), false);
            }
            // Sitting in the tray: after five minutes, release the window (and WebView2's memory).
            // It's rebuilt in a moment when Marquee is opened again.
            match app.get_webview_window("main") {
                Some(w) if !w.is_visible().unwrap_or(true) => {
                    hidden_for += 1;
                    if hidden_for >= 5 && store::setting_or(&ctx.db, "tray.release_window", true) && ctx.busy_reason().is_none() {
                        let _ = w.destroy();
                        if let Some(panel) = app.get_webview_window("tray") {
                            let _ = panel.destroy();
                        }
                        hidden_for = 0;
                    }
                }
                _ => hidden_for = 0,
            }
        }
    });
}

/// Minutes since an RFC 3339 timestamp.
fn chrono_since(at: &str) -> Option<f64> {
    let then = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(parse_rfc3339_secs(at)?);
    std::time::SystemTime::now().duration_since(then).ok().map(|d| d.as_secs_f64() / 60.0)
}

/// Seconds since the epoch for "YYYY-MM-DDTHH:MM:SSZ" (the only form Marquee writes).
fn parse_rfc3339_secs(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d, h, mi, se) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    // Days from civil (Howard Hinnant's algorithm).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + h * 3600 + mi * 60 + se).ok()
}

pub fn run() {
    let args: Vec<String> = std::env::args().collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let (flag_minimized, flag_autostart, flag_quit) = (has("--minimized"), has("--autostart"), has("--quit"));

    let builder = tauri::Builder::default()
        // A second launch focuses the running copy (and passes along --quit or a file to open).
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if argv.iter().any(|a| a == "--quit") {
                quit(app);
                return;
            }
            let file = argv.iter().skip(1).find(|a| a.ends_with(".marqueetheme") || a.ends_with(".marqueebackup")).cloned();
            tray::show_main(app, None);
            if let Some(file) = file {
                let _ = app.emit("file:opened", file);
            }
        }))
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(tauri_plugin_window_state::StateFlags::SIZE | tauri_plugin_window_state::StateFlags::POSITION | tauri_plugin_window_state::StateFlags::MAXIMIZED)
                .with_denylist(&["tray"])
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--autostart"])))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .register_asynchronous_uri_scheme_protocol("mqimg", |ctx, request, responder| serve_image(ctx.app_handle().clone(), request, responder))
        .manage(Quitting(AtomicBool::new(false)))
        .manage(PendingRoute(Mutex::new(None)))
        .manage(updates::Updates::default())
        .manage(Arc::new(hangar_pipe::UiState::default()))
        .invoke_handler(tauri::generate_handler![
            call,
            app_ready,
            system_info,
            clear_crash,
            set_backdrop,
            set_ui_state,
            open_url,
            open_folder,
            quit_app,
            save_file,
            set_data_location,
            tray::tray_hide,
            tray::open_main,
            updates::update_check,
            updates::update_state,
            updates::update_install,
            updates::update_roll_back,
        ])
        .on_window_event(|window, event| match event {
            // Closing the main window sends Marquee to the tray; Quit is in the tray menu.
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                let app = window.app_handle();
                let quitting = app.state::<Quitting>().0.load(Ordering::SeqCst);
                let to_tray = app.try_state::<Arc<Ctx>>().map(|c| store::setting_or(&c.db, "close_to_tray", true)).unwrap_or(true);
                if !quitting && to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                } else if !quitting {
                    quit(app);
                }
            }
            WindowEvent::Focused(false) if window.label() == "tray" => {
                let _ = window.hide();
            }
            _ => {}
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            let fallback = app.path().app_local_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("Marquee"));
            let (data_dir, portable) = system::data_dir(&fallback);
            std::fs::create_dir_all(&data_dir)?;
            let log_guard = system::init_logging(&data_dir);
            system::install_panic_hook(data_dir.clone());
            let version = app.package_info().version.to_string();
            tracing::info!("Marquee {version} starting (data in {})", data_dir.display());

            // If this version has failed to start twice, go back to the one before it.
            if let updater::Boot::RollBack { to_version, installer } = updater::on_start(&data_dir, &version) {
                tracing::error!("{version} didn't start properly twice; rolling back to {to_version}");
                if updates::run_installer(&installer).is_ok() {
                    std::process::exit(0);
                }
            }

            let db = marquee_core::db::Db::open(&data_dir.join("marquee.db")).map_err(|e| {
                use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
                handle.dialog().message(format!("Marquee couldn't open its data.\n\n{e}\n\nLogs are in {}", data_dir.join("logs").display())).kind(MessageDialogKind::Error).title("Marquee").blocking_show();
                e
            })?;
            let user_agent = format!("Marquee/{version} ( https://github.com/larosa023/Marquee )");
            let http = Http::new(Box::new(ReqwestTransport::new(&user_agent)?), db.clone());
            let ctx = Arc::new(Ctx::new(db, http, system::secret_store(), Arc::new(TauriHost(handle.clone())), data_dir.clone(), &version, system::built_in_keys()));

            // `--quit` with nothing running (the single-instance plugin handles the running case).
            if flag_quit || run_cli(&args, &ctx) {
                std::process::exit(0);
            }

            let crashed = std::fs::read_to_string(data_dir.join("crash.txt")).ok();
            let restore_state = hangar::restore_state_arg(&args).and_then(|f| hangar::take_restore_state(std::path::Path::new(&f)).ok()).or_else(|| updates::take_saved_state(&data_dir));
            if restore_state.is_some() {
                hangar_pipe::send(hangar::restored_message("back where you were"));
            }
            let start_in_tray = store::setting_or(&ctx.db, "start_in_tray", false);
            app.manage(Launch {
                hidden: flag_minimized || (flag_autostart && start_in_tray),
                restore_state: Mutex::new(restore_state),
                portable,
                default_data_dir: system::default_data_dir(&fallback),
                crashed: Mutex::new(crashed),
                _log_guard: log_guard,
            });
            app.manage(ctx.clone());

            if let Err(e) = tray::build(&handle) {
                tracing::warn!("the tray icon couldn't be created: {e}");
            }
            hangar_pipe::start(handle.clone(), ctx.clone(), app.state::<Arc<hangar_pipe::UiState>>().inner().clone());
            start_background(handle, ctx);
            Ok(())
        });

    let app = match builder.build(tauri::generate_context!()) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Marquee couldn't start: {e}");
            std::process::exit(1);
        }
    };
    app.run(|app, event| match event {
        // With every window closed Marquee keeps running in the tray.
        RunEvent::ExitRequested { api, code, .. } => {
            if code.is_none() && !app.state::<Quitting>().0.load(Ordering::SeqCst) {
                api.prevent_exit();
            }
        }
        RunEvent::Exit => hangar_pipe::cleanup(),
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_parse_to_epoch_seconds() {
        assert_eq!(parse_rfc3339_secs("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339_secs("2023-11-14T22:13:20Z"), Some(1_700_000_000));
        assert_eq!(parse_rfc3339_secs("2024-02-29T12:00:00Z"), Some(1_709_208_000));
        assert_eq!(parse_rfc3339_secs("nonsense"), None);
    }
}
