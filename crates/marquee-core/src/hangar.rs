//! Hangar integration (protocol v1, see docs/IPC.md in larosa023/Hangar).
//!
//! This module is the platform-neutral half: what Marquee answers to each Hangar request, the
//! discovery file, and the manifest entry. The named-pipe transport lives in the app crate.
//!
//! Marquee answers the standard requests (`hello`, `status`, `prepare-update`, `restart`,
//! `cancel-update`, `apply-theme`, `apply-settings`, `export-settings`) and three extra ones so
//! Hangar can drive backups and updates directly: `backup`, `restore` and `check-update`.

use crate::backup;
use crate::ctx::Ctx;
use crate::error::Result;
use crate::store;
use crate::themes;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const APP_ID: &str = "marquee";
pub const PROTOCOL: u32 = 1;

pub fn pipe_name(session_id: u32) -> String {
    format!(r"\\.\pipe\larosa023.{APP_ID}.{session_id}")
}

pub fn hangar_pipe_name(session_id: u32) -> String {
    format!(r"\\.\pipe\larosa023.hangar.{session_id}")
}

/// `%LOCALAPPDATA%\Hangar`
pub fn hangar_dir(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Hangar")
}

pub fn installed(local_app_data: &Path) -> bool {
    hangar_dir(local_app_data).is_dir() || local_app_data.join("Programs").join("Hangar").join("Hangar.exe").is_file()
}

/// The file Hangar reads to learn that a running app speaks the protocol.
pub fn run_file(local_app_data: &Path) -> PathBuf {
    hangar_dir(local_app_data).join("run").join(format!("{APP_ID}.json"))
}

pub fn run_file_contents(pid: u32, session_id: u32, version: &str, exe: &str) -> Value {
    json!({ "pid": pid, "pipe": pipe_name(session_id), "version": version, "protocol": PROTOCOL, "exe": exe })
}

/// Should Marquee leave updating to Hangar (and hide its own update prompts)?
pub fn manages_updates(ctx: &Ctx, hangar_installed: bool) -> bool {
    hangar_installed && store::setting_or(&ctx.db, "updates.defer_to_hangar", true)
}

/// The entry for Hangar's `manifest.json`. Tauri apps in the suite install with NSIS.
pub fn manifest_entry() -> Value {
    json!({
        "id": APP_ID,
        "name": "Marquee",
        "repo": "larosa023/Marquee",
        "description": "New movies, shows, games and albums, picked for your taste.",
        "category": "media",
        "icon": "https://raw.githubusercontent.com/larosa023/Marquee/main/src-tauri/icons/icon.png",
        "exe": "Marquee.exe",
        "install": { "type": "nsis", "asset": "Marquee_*_x64-setup.exe", "registry": "Marquee" },
        "args": { "quit": "--quit" },
        "liveUpdate": true
    })
}

/// What the app must do after a request has been answered.
#[derive(Debug, PartialEq)]
pub enum After {
    Nothing,
    /// Hangar sent `restart`: exit so the new version can start.
    Exit,
    /// Hangar asked for an update check.
    CheckUpdate,
}

/// Answers one request from Hangar. `ui_state` is what the window wants restored after a live
/// update (current page, item, search, scroll).
pub fn handle(ctx: &Ctx, req: &Value, ui_state: &Value) -> (Value, After) {
    let kind = req["type"].as_str().unwrap_or("");
    let mut after = After::Nothing;
    let mut reply = match kind {
        "hello" => json!({ "protocol": PROTOCOL, "app": APP_ID, "version": ctx.version, "capabilities": ["state", "theme", "settings", "backup", "update"] }),
        "status" => match ctx.busy_reason() {
            Some(reason) => json!({ "state": "busy", "reason": reason }),
            None => json!({ "state": "idle" }),
        },
        "prepare-update" => match ctx.busy_reason() {
            // An import or restore in progress must finish first; Hangar treats a refusal as "busy".
            Some(reason) if reason != "syncing" => json!({ "ok": false, "reason": reason }),
            _ => {
                let file = ctx.data_dir.join("restore-state.json");
                match std::fs::write(&file, ui_state.to_string()) {
                    Ok(()) => json!({ "ok": true, "stateFile": file.to_string_lossy() }),
                    Err(e) => json!({ "ok": false, "reason": format!("couldn't save state: {e}") }),
                }
            }
        },
        "cancel-update" => {
            let _ = std::fs::remove_file(ctx.data_dir.join("restore-state.json"));
            json!({ "ok": true })
        }
        "restart" => {
            after = After::Exit;
            json!({ "ok": true })
        }
        "apply-theme" => match themes::from_suite_json(&req["theme"]) {
            Ok(theme) => {
                let stored = store::set_setting(&ctx.db, "hangar.theme", &theme);
                ctx.emit("theme:hangar", json!({ "theme": theme }));
                json!({ "ok": stored.is_ok() })
            }
            Err(e) => json!({ "ok": false, "reason": e.to_string() }),
        },
        "apply-settings" => {
            // Settings live in the database, which a restore has already replaced: just tell the UI.
            ctx.emit("settings:changed", json!({}));
            json!({ "ok": true, "restartRequired": false })
        }
        "export-settings" | "backup" => {
            let dest = req["file"].as_str().map(PathBuf::from);
            match backup::create(ctx, dest.as_deref(), false) {
                Ok(file) => json!({ "ok": true, "file": file.to_string_lossy() }),
                Err(e) => json!({ "ok": false, "reason": e.to_string() }),
            }
        }
        "restore" => match req["file"].as_str() {
            Some(file) => match backup::restore(ctx, Path::new(file)) {
                Ok(_) => json!({ "ok": true, "restartRequired": false }),
                Err(e) => json!({ "ok": false, "reason": e.to_string() }),
            },
            None => json!({ "ok": false, "reason": "no file given" }),
        },
        "check-update" => {
            after = After::CheckUpdate;
            json!({ "ok": true })
        }
        _ => json!({ "ok": false, "reason": "unknown request" }),
    };
    if let (Some(id), Some(obj)) = (req.get("id"), reply.as_object_mut()) {
        obj.insert("id".into(), id.clone());
    }
    (reply, after)
}

/// Messages Marquee sends to Hangar's own pipe.
pub fn health_message(version: &str, pid: u32) -> Value {
    json!({ "type": "health", "app": APP_ID, "version": version, "pid": pid })
}

pub fn restored_message(summary: &str) -> Value {
    json!({ "type": "restored", "app": APP_ID, "summary": summary })
}

pub fn busy_message(busy: bool, reason: &str) -> Value {
    if busy { json!({ "type": "busy", "app": APP_ID, "reason": reason }) } else { json!({ "type": "idle", "app": APP_ID }) }
}

/// Reads and removes the state saved before a live update (`--restore-state <file>`).
pub fn take_restore_state(file: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(file)?;
    let _ = std::fs::remove_file(file);
    Ok(serde_json::from_str(&text)?)
}

/// `--restore-state <file>` from the command line, if present.
pub fn restore_state_arg(args: &[String]) -> Option<String> {
    args.windows(2).find(|w| w[0] == "--restore-state").map(|w| w[1].clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testing::ctx;

    #[test]
    fn names_follow_the_suite_convention() {
        assert_eq!(pipe_name(1), r"\\.\pipe\larosa023.marquee.1");
        assert_eq!(hangar_pipe_name(3), r"\\.\pipe\larosa023.hangar.3");
        let run = run_file_contents(1234, 1, "0.1.0", r"C:\Apps\Marquee.exe");
        assert_eq!(run["protocol"], 1);
        assert_eq!(run["pipe"], r"\\.\pipe\larosa023.marquee.1");
        assert!(run_file(Path::new(r"C:\Users\a\AppData\Local")).to_string_lossy().contains("Hangar"));
    }

    #[test]
    fn the_manifest_entry_has_what_hangar_requires() {
        let e = manifest_entry();
        // The checks Hangar's own CI runs on manifest.json.
        for key in ["id", "name", "repo", "exe", "install"] {
            assert!(e.get(key).is_some(), "missing {key}");
        }
        assert!(["portable", "nsis", "velopack"].contains(&e["install"]["type"].as_str().unwrap()));
        assert!(["windows", "audio", "gaming", "media", "work"].contains(&e["category"].as_str().unwrap()));
        assert_eq!(e["id"], e["id"].as_str().unwrap().to_lowercase());
    }

    #[test]
    fn hello_and_status_echo_the_request_id() {
        let t = ctx();
        let (hello, after) = handle(&t.ctx, &json!({ "id": 7, "type": "hello" }), &Value::Null);
        assert_eq!(after, After::Nothing);
        assert_eq!((hello["id"].as_i64(), hello["protocol"].as_i64(), hello["app"].as_str()), (Some(7), Some(1), Some("marquee")));
        assert!(hello["capabilities"].as_array().unwrap().contains(&json!("theme")));
        assert_eq!(handle(&t.ctx, &json!({ "id": 8, "type": "status" }), &Value::Null).0["state"], "idle");
        t.ctx.set_busy(Some("importing"));
        let (busy, _) = handle(&t.ctx, &json!({ "id": 9, "type": "status" }), &Value::Null);
        assert_eq!((busy["state"].as_str(), busy["reason"].as_str()), (Some("busy"), Some("importing")));
    }

    #[test]
    fn a_live_update_saves_state_and_restart_asks_to_exit() {
        let t = ctx();
        let ui = json!({ "route": "/item/42", "search": "arrival", "scroll": 380 });
        let (reply, _) = handle(&t.ctx, &json!({ "id": 1, "type": "prepare-update" }), &ui);
        assert_eq!(reply["ok"], true);
        let file = PathBuf::from(reply["stateFile"].as_str().unwrap());
        assert_eq!(take_restore_state(&file).unwrap(), ui);
        assert!(!file.exists(), "the state file is used once");
        assert_eq!(handle(&t.ctx, &json!({ "id": 2, "type": "restart" }), &Value::Null).1, After::Exit);
        // An import in progress refuses; a sync doesn't (it resumes after the restart).
        t.ctx.set_busy(Some("importing"));
        assert_eq!(handle(&t.ctx, &json!({ "type": "prepare-update" }), &ui).0["ok"], false);
        t.ctx.set_busy(Some("syncing"));
        assert_eq!(handle(&t.ctx, &json!({ "type": "prepare-update" }), &ui).0["ok"], true);
    }

    #[test]
    fn hangar_can_push_a_suite_theme() {
        let t = ctx();
        let theme = json!({ "schema": 1, "name": "Frost", "mode": "dark", "backdrop": "mica", "colors": { "background": "#2E3440", "sidebar": "#272C36", "surface": "#3B4252", "surfaceHover": "#434C5E", "surfaceStrong": "#4C566A", "text": "#ECEFF4", "subtext": "#C0C8D6", "faint": "#7B8698", "accent": "#88C0D0" } });
        let (reply, _) = handle(&t.ctx, &json!({ "id": 3, "type": "apply-theme", "theme": theme }), &Value::Null);
        assert_eq!(reply["ok"], true);
        let stored: themes::Theme = store::get_setting(&t.ctx.db, "hangar.theme").unwrap();
        assert_eq!(stored.colors.accent, "#88C0D0");
        assert!(t.host.events.lock().unwrap().iter().any(|(e, p)| e == "theme:hangar" && p["theme"]["name"] == "Frost"));
        let (bad, _) = handle(&t.ctx, &json!({ "type": "apply-theme", "theme": { "name": "x", "colors": { "text": "nope" } } }), &Value::Null);
        assert_eq!(bad["ok"], false);
    }

    #[test]
    fn hangar_can_trigger_backup_restore_and_update() {
        let t = ctx();
        store::set_setting(&t.ctx.db, "region", &"US").unwrap();
        let dest = t.dir.join("from-hangar.marqueebackup");
        let (reply, _) = handle(&t.ctx, &json!({ "id": 4, "type": "export-settings", "file": dest.to_string_lossy() }), &Value::Null);
        assert_eq!(reply["ok"], true);
        assert!(dest.exists());
        store::set_setting(&t.ctx.db, "region", &"GB").unwrap();
        let (restored, _) = handle(&t.ctx, &json!({ "id": 5, "type": "restore", "file": dest.to_string_lossy() }), &Value::Null);
        assert_eq!(restored["ok"], true);
        assert_eq!(store::get_setting::<String>(&t.ctx.db, "region").as_deref(), Some("US"));
        assert_eq!(handle(&t.ctx, &json!({ "type": "check-update" }), &Value::Null).1, After::CheckUpdate);
        assert_eq!(handle(&t.ctx, &json!({ "type": "dance" }), &Value::Null).0["ok"], false);
    }

    #[test]
    fn updates_defer_to_hangar_only_when_it_is_installed_and_allowed() {
        let t = ctx();
        assert!(!manages_updates(&t.ctx, false));
        assert!(manages_updates(&t.ctx, true));
        store::set_setting(&t.ctx.db, "updates.defer_to_hangar", &false).unwrap();
        assert!(!manages_updates(&t.ctx, true));
        assert_eq!(restore_state_arg(&["Marquee.exe".into(), "--restore-state".into(), "C:\\s.json".into()]).as_deref(), Some("C:\\s.json"));
        assert_eq!(restore_state_arg(&["Marquee.exe".into()]), None);
    }
}
