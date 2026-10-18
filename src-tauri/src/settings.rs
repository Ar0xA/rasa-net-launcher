use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Credential Manager (Windows) / Secret Service (Linux) name for saved passwords.
const CREDENTIAL_SERVICE: &str = "org.infiniterasa.launcher";

#[derive(Serialize, Deserialize, Clone)]
pub struct Server {
    pub id: String,
    pub name: String,
    /// Auth server as host:port, passed to the client as /AuthServer=.
    pub host: String,
    #[serde(default)]
    pub username: String,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    pub game_dir: Option<PathBuf>,
    /// Folder that last passed a full (hashed) verify; a different folder needs one first.
    pub verified_dir: Option<PathBuf>,
    pub servers: Vec<Server>,
    pub selected_server: String,
    pub close_on_launch: bool,
    /// How the Windows client is started outside Windows.
    #[cfg(not(windows))]
    pub launch: crate::runner::LaunchConfig,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            game_dir: None,
            verified_dir: None,
            servers: vec![Server {
                id: "infinite-rasa".into(),
                name: "Infinite Rasa".into(),
                host: "game.infiniterasa.org:2106".into(),
                username: String::new(),
            }],
            selected_server: "infinite-rasa".into(),
            close_on_launch: false,
            #[cfg(not(windows))]
            launch: Default::default(),
        }
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("settings.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    settings_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

fn credential(server_id: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(CREDENTIAL_SERVICE, server_id).map_err(|e| e.to_string())
}

/// The saved password for a server, if the player chose to remember one.
pub fn password(server_id: &str) -> Option<String> {
    credential(server_id).ok()?.get_password().ok()
}

/// True if a keyring can hold passwords. Outside Windows that needs a running
/// Secret Service (GNOME Keyring, KWallet, KeePassXC, …). When it is false the
/// settings dialog does not accept a password at all, so none is ever stored in clear text.
#[cfg(not(windows))]
pub fn can_save_passwords() -> bool {
    credential("keyring-check")
        .is_ok_and(|e| matches!(e.get_password(), Ok(_) | Err(keyring::Error::NoEntry)))
}

/// Stores the password, or forgets it when `password` is None.
pub fn set_password(server_id: &str, password: Option<&str>) -> Result<(), String> {
    let entry = credential(server_id)?;
    match password {
        Some(p) => entry.set_password(p).map_err(|e| e.to_string()),
        None => match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        },
    }
}
