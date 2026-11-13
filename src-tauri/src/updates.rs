//! The built-in updater: signed updates from GitHub Releases, Stable and Beta channels,
//! "install now" or "on next launch", and rollback bookkeeping (see `marquee_core::updater`).
//!
//! When Hangar is installed and managing updates, Marquee doesn't check or prompt on its own:
//! Hangar does the updating, and asks Marquee over the pipe when it wants a check.

use marquee_core::{hangar, store, updater, Ctx};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct Updates {
    found: Mutex<Option<Update>>,
    info: Mutex<Option<UpdateInfo>>,
    busy: std::sync::atomic::AtomicBool,
}

#[derive(Serialize, Clone, Debug)]
pub struct UpdateInfo {
    pub current: String,
    pub version: String,
    pub notes: String,
    pub prerelease: bool,
    pub date: Option<String>,
}

pub fn hangar_installed() -> bool {
    crate::system::local_app_data().map(|l| hangar::installed(&l)).unwrap_or(false)
}

/// True when Marquee should stay quiet about updates and let Hangar handle them.
pub fn deferred_to_hangar(ctx: &Ctx) -> bool {
    hangar::manages_updates(ctx, hangar_installed())
}

fn channel(ctx: &Ctx) -> String {
    store::setting_or(&ctx.db, "updates.channel", "stable".to_string())
}

/// Looks for a newer version on the user's channel. Beta checks both manifests and takes the newer.
async fn find(app: &AppHandle) -> Result<Option<Update>, String> {
    let ctx = app.state::<Arc<Ctx>>().inner().clone();
    let skip = updater::skipped(&ctx.data_dir);
    let mut best: Option<Update> = None;
    let mut last_error = None;
    for endpoint in updater::endpoints(&channel(&ctx)) {
        let url = url::Url::parse(&endpoint).map_err(|e| e.to_string())?;
        let checker = app.updater_builder().endpoints(vec![url]).map_err(|e| e.to_string())?.build().map_err(|e| e.to_string())?;
        match checker.check().await {
            Ok(Some(update)) => {
                if skip.contains(&update.version) {
                    tracing::info!("skipping {} (it was rolled back on this PC)", update.version);
                    continue;
                }
                if best.as_ref().map(|b| updater::is_newer(&update.version, &b.version)).unwrap_or(true) {
                    best = Some(update);
                }
            }
            Ok(None) => {}
            // The beta manifest doesn't exist until the first beta is published.
            Err(e) => last_error = Some(e.to_string()),
        }
    }
    match (best, last_error) {
        (Some(u), _) => Ok(Some(u)),
        (None, Some(e)) if channel(&ctx) != "beta" => Err(format!("Couldn't check for updates: {e}")),
        _ => Ok(None),
    }
}

async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let found = find(app).await?;
    let state = app.state::<Updates>();
    let info =
        found.as_ref().map(|u| UpdateInfo { current: u.current_version.clone(), version: u.version.clone(), notes: u.body.clone().unwrap_or_default(), prerelease: updater::is_prerelease(&u.version), date: u.date.map(|d| d.to_string()) });
    *state.found.lock().unwrap() = found;
    *state.info.lock().unwrap() = info.clone();
    if let Some(i) = &info {
        let _ = app.emit("update:available", i);
    }
    Ok(info)
}

/// Checks without bothering the user if nothing is found. `forced` checks even when Hangar manages updates.
pub fn check_in_background(app: AppHandle, forced: bool) {
    tauri::async_runtime::spawn(async move {
        let ctx = app.state::<Arc<Ctx>>().inner().clone();
        if !forced && deferred_to_hangar(&ctx) {
            return;
        }
        match check(&app).await {
            Ok(Some(info)) => {
                tracing::info!("update {} is available", info.version);
                // "On next launch" from a previous session: install before anything else.
                let pending: Option<String> = store::get_setting(&ctx.db, "updates.install_on_launch");
                if pending.as_deref() == Some(info.version.as_str()) {
                    let _ = store::set_setting(&ctx.db, "updates.install_on_launch", &Value::Null);
                    if let Err(e) = install(&app).await {
                        tracing::warn!("deferred update failed: {e}");
                    }
                }
            }
            Ok(None) => {}
            Err(e) => tracing::warn!("{e}"),
        }
        let _ = store::set_setting(&ctx.db, "updates.last_check", &marquee_core::model::now());
    });
}

/// Saves the installer for the running version so a bad update can be undone.
async fn save_rollback_installer(ctx: &Ctx) -> Option<std::path::PathBuf> {
    let dir = ctx.data_dir.join("rollback");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("Marquee_{}_x64-setup.exe", ctx.version));
    if path.is_file() {
        return Some(path);
    }
    // A plain download from this repo's own releases; it isn't an API call, so it bypasses the JSON scheduler cache.
    let resp = ctx.http.get("GitHub", updater::installer_url(&ctx.version)).send().await.ok()?;
    if resp.body.len() < 1024 * 1024 {
        return None; // not an installer (a 404 page, say)
    }
    std::fs::write(&path, &resp.body).ok()?;
    // Keep only this one: older rollback installers are no longer useful.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for e in entries.flatten() {
            if e.path() != path {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    Some(path)
}

async fn install(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<Updates>();
    if state.busy.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err("An update is already installing.".into());
    }
    let update = state.found.lock().unwrap().take();
    let Some(update) = update else {
        state.busy.store(false, std::sync::atomic::Ordering::SeqCst);
        return Err("Check for updates first.".into());
    };
    let ctx = app.state::<Arc<Ctx>>().inner().clone();
    if let Some(reason) = ctx.busy_reason().filter(|r| r != "syncing") {
        state.busy.store(false, std::sync::atomic::Ordering::SeqCst);
        *state.found.lock().unwrap() = Some(update);
        return Err(format!("Marquee is busy ({reason}). Try again when it's done."));
    }
    let installer = save_rollback_installer(&ctx).await;
    updater::before_update(&ctx.data_dir, &ctx.version, installer.as_deref());
    save_ui_state(app, ctx.as_ref());

    let progress = app.clone();
    let mut downloaded: u64 = 0;
    let result = update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ = progress.emit("update:progress", json!({ "downloaded": downloaded, "total": total }));
            },
            || {},
        )
        .await;
    state.busy.store(false, std::sync::atomic::Ordering::SeqCst);
    match result {
        // On Windows the installer takes over and relaunches Marquee; elsewhere, restart ourselves.
        Ok(()) => {
            app.state::<crate::Quitting>().0.store(true, std::sync::atomic::Ordering::SeqCst);
            app.restart();
        }
        Err(e) => Err(format!("The update couldn't be installed: {e}")),
    }
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let ctx = app.state::<Arc<Ctx>>().inner().clone();
    let result = check(&app).await;
    let _ = store::set_setting(&ctx.db, "updates.last_check", &marquee_core::model::now());
    result
}

#[tauri::command]
pub fn update_state(app: AppHandle) -> Value {
    let ctx = app.state::<Arc<Ctx>>().inner().clone();
    let state = app.state::<Updates>();
    let boot = updater::load(&ctx.data_dir);
    json!({
        "available": state.info.lock().unwrap().clone(),
        "channel": channel(&ctx),
        "last_check": store::get_setting::<String>(&ctx.db, "updates.last_check"),
        "hangar_installed": hangar_installed(),
        "deferred_to_hangar": deferred_to_hangar(&ctx),
        "install_on_launch": store::get_setting::<String>(&ctx.db, "updates.install_on_launch"),
        "skipped": boot.skip,
        "can_roll_back": boot.previous_installer.as_ref().map(|p| std::path::Path::new(p).is_file()).unwrap_or(false),
        "previous_version": boot.previous_version,
    })
}

/// `when`: "now" installs and restarts; "next" installs the next time Marquee starts.
#[tauri::command]
pub async fn update_install(app: AppHandle, when: String) -> Result<(), String> {
    if when == "next" {
        let ctx = app.state::<Arc<Ctx>>().inner().clone();
        let version = app.state::<Updates>().info.lock().unwrap().as_ref().map(|i| i.version.clone()).ok_or("Check for updates first.")?;
        store::set_setting(&ctx.db, "updates.install_on_launch", &version).map_err(|e| e.to_string())?;
        return Ok(());
    }
    install(&app).await
}

/// Runs the saved installer for the previous version (Settings → Updates → Roll back).
#[tauri::command]
pub fn update_roll_back(app: AppHandle) -> Result<(), String> {
    let ctx = app.state::<Arc<Ctx>>().inner().clone();
    let boot = updater::load(&ctx.data_dir);
    let installer = boot.previous_installer.filter(|p| std::path::Path::new(p).is_file()).ok_or("There's no earlier version saved to roll back to.")?;
    run_installer(std::path::Path::new(&installer))?;
    app.state::<crate::Quitting>().0.store(true, std::sync::atomic::Ordering::SeqCst);
    app.exit(0);
    Ok(())
}

/// Remembers the current screen across a self-update, so the new version opens where this one left off.
fn save_ui_state(app: &AppHandle, ctx: &Ctx) {
    let state = app.state::<Arc<crate::hangar_pipe::UiState>>().0.lock().map(|s| s.clone()).unwrap_or(Value::Null);
    if !state.is_null() {
        let _ = std::fs::write(ctx.data_dir.join("restore-state.json"), state.to_string());
    }
}

/// The screen saved by `save_ui_state`, if this start follows an update within the last few minutes.
pub fn take_saved_state(data_dir: &std::path::Path) -> Option<Value> {
    let file = data_dir.join("restore-state.json");
    let fresh = std::fs::metadata(&file).ok()?.modified().ok()?.elapsed().map(|age| age.as_secs() < 900).unwrap_or(false);
    let state = marquee_core::hangar::take_restore_state(&file).ok()?;
    fresh.then_some(state)
}

/// Starts an NSIS installer in passive mode (progress only) and asks it to relaunch Marquee.
pub fn run_installer(installer: &std::path::Path) -> Result<(), String> {
    std::process::Command::new(installer).args(["/P", "/R"]).spawn().map(|_| ()).map_err(|e| format!("Couldn't start the installer: {e}"))
}
