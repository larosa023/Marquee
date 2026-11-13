//! Things that depend on the machine: where data lives, logging, secrets, the Windows accent color.

use marquee_core::secrets::SecretStore;
use std::path::{Path, PathBuf};

/// Where Marquee keeps its data.
///
/// * portable mode (a file named `portable` next to the exe): `.\data`, like the rest of the suite
/// * a custom location chosen in Settings → Storage (recorded in `location.txt`)
/// * otherwise `%LOCALAPPDATA%\com.larosa023.marquee\data`
pub fn data_dir(fallback: &Path) -> (PathBuf, bool) {
    if let Some(exe_dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        if exe_dir.join("portable").exists() {
            return (exe_dir.join("data"), true);
        }
    }
    let default = default_data_dir(fallback);
    if let Ok(custom) = std::fs::read_to_string(default.join("location.txt")) {
        let custom = PathBuf::from(custom.trim());
        if custom.is_absolute() && custom.is_dir() {
            return (custom, false);
        }
    }
    (default, false)
}

pub fn default_data_dir(app_local_data: &Path) -> PathBuf {
    // Not `%LOCALAPPDATA%\Marquee`: that is where the per-user installer puts the program itself.
    // The app's own folder is also what the uninstaller's "delete application data" box removes.
    app_local_data.join("data")
}

pub fn local_app_data() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
}

/// Rotating daily log files in `<data>/logs`, newest seven kept. Nothing is ever sent anywhere.
pub fn init_logging(data_dir: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let dir = data_dir.join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    let appender = tracing_appender::rolling::Builder::new().rotation(tracing_appender::rolling::Rotation::DAILY).filename_prefix("marquee").filename_suffix("log").max_log_files(7).build(&dir).ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let level = if cfg!(debug_assertions) { tracing::Level::DEBUG } else { tracing::Level::INFO };
    let _ = tracing_subscriber::fmt().with_writer(writer).with_ansi(false).with_max_level(level).with_target(false).try_init();
    Some(guard)
}

/// Records a crash so the next launch can show where the log is.
pub fn install_panic_hook(data_dir: PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let message = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_else(|| "unknown error".into());
        tracing::error!("panic at {location}: {message}");
        let _ = std::fs::write(data_dir.join("crash.txt"), format!("{}\n{message}\n{location}\n", marquee_core::model::now()));
        default(info);
    }));
}

/// The Windows accent color as "#RRGGBB", for the "Follow Windows" theme.
pub fn accent_color() -> Option<String> {
    #[cfg(windows)]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        let key = winreg::RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Microsoft\Windows\DWM").ok()?;
        // Stored as 0xAABBGGRR.
        let v: u32 = key.get_value("AccentColor").ok()?;
        Some(format!("#{:02X}{:02X}{:02X}", v & 0xFF, (v >> 8) & 0xFF, (v >> 16) & 0xFF))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Secrets in Windows Credential Manager (under the service name "Marquee").
#[cfg(windows)]
pub struct Keyring;

#[cfg(windows)]
impl SecretStore for Keyring {
    fn get(&self, key: &str) -> Option<String> {
        keyring::Entry::new("Marquee", key).ok()?.get_password().ok()
    }
    fn set(&self, key: &str, value: &str) -> marquee_core::Result<()> {
        keyring::Entry::new("Marquee", key).and_then(|e| e.set_password(value)).map_err(|e| marquee_core::Error::msg(format!("Windows Credential Manager wouldn't store that: {e}")))
    }
    fn delete(&self, key: &str) -> marquee_core::Result<()> {
        match keyring::Entry::new("Marquee", key).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(marquee_core::Error::msg(format!("Windows Credential Manager wouldn't remove that: {e}"))),
        }
    }
}

pub fn secret_store() -> std::sync::Arc<dyn SecretStore> {
    #[cfg(windows)]
    {
        std::sync::Arc::new(Keyring)
    }
    #[cfg(not(windows))]
    {
        // Marquee ships for Windows only. Elsewhere (development) secrets live in memory.
        std::sync::Arc::new(marquee_core::secrets::MemorySecrets::new())
    }
}

/// The API keys baked in at build time from `.env` (see build.rs).
pub fn built_in_keys() -> std::collections::HashMap<String, String> {
    [
        ("tmdb", env!("MARQUEE_TMDB_API_KEY")),
        ("igdb_client_id", env!("MARQUEE_IGDB_CLIENT_ID")),
        ("igdb_client_secret", env!("MARQUEE_IGDB_CLIENT_SECRET")),
        ("lastfm_key", env!("MARQUEE_LASTFM_API_KEY")),
        ("lastfm_secret", env!("MARQUEE_LASTFM_API_SECRET")),
        ("spotify_client_id", env!("MARQUEE_SPOTIFY_CLIENT_ID")),
        ("trakt_client_id", env!("MARQUEE_TRAKT_CLIENT_ID")),
        ("trakt_client_secret", env!("MARQUEE_TRAKT_CLIENT_SECRET")),
        ("steam", env!("MARQUEE_STEAM_API_KEY")),
        ("itad", env!("MARQUEE_ITAD_API_KEY")),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

/// Moves the data folder: copies the database, themes and backups to `dest` and records the
/// new location. The app restarts afterwards to open it.
pub fn move_data(current: &Path, default: &Path, dest: &Path) -> marquee_core::Result<()> {
    if !dest.is_absolute() {
        return Err(marquee_core::Error::msg("Choose a full folder path."));
    }
    std::fs::create_dir_all(dest)?;
    if dest.join("marquee.db").exists() && dest != current {
        return Err(marquee_core::Error::msg("That folder already has Marquee data in it. Choose an empty folder."));
    }
    fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            let target = to.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_dir(&entry.path(), &target)?;
            } else {
                std::fs::copy(entry.path(), target)?;
            }
        }
        Ok(())
    }
    for name in ["marquee.db", "boot.json"] {
        let src = current.join(name);
        if src.exists() {
            std::fs::copy(&src, dest.join(name))?;
        }
    }
    for sub in ["themes", "backups"] {
        let src = current.join(sub);
        if src.is_dir() {
            copy_dir(&src, &dest.join(sub))?;
        }
    }
    std::fs::create_dir_all(default)?;
    if dest == default {
        let _ = std::fs::remove_file(default.join("location.txt"));
    } else {
        std::fs::write(default.join("location.txt"), dest.to_string_lossy().as_bytes())?;
    }
    Ok(())
}
