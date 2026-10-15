use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::{AppHandle, Emitter, Manager};

/// Auth server the client connects to. Change this for live.
const AUTH_SERVER: &str = "test.game.infiniterasa.org:2106";
const GAME_EXE: &str = "tabula_rasa.exe";

/// Reference hashes of a clean 1.16.5.0 client (tools/New-Manifest.ps1).
const MANIFEST_JSON: &str = include_str!("../manifest.json");

#[derive(Deserialize)]
struct Manifest {
    version: String,
    files: Vec<ManifestEntry>,
}

#[derive(Deserialize)]
struct ManifestEntry {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Serialize, Deserialize, Default)]
struct Settings {
    game_dir: Option<PathBuf>,
}

#[derive(Serialize, Clone)]
struct Progress {
    done: usize,
    total: usize,
}

#[derive(Serialize)]
struct VerifyResult {
    version: String,
    checked: usize,
    missing: Vec<String>,
    corrupt: Vec<String>,
}

fn manifest() -> Result<Manifest, String> {
    // PowerShell writes a UTF-8 BOM; serde_json rejects it.
    serde_json::from_str(MANIFEST_JSON.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("Bad embedded manifest: {e}"))
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    Ok(dir.join("settings.json"))
}

fn load_settings(app: &AppHandle) -> Settings {
    settings_path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, json).map_err(|e| e.to_string())
}

fn is_game_dir(dir: &Path) -> bool {
    dir.join(GAME_EXE).is_file()
}

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Install locations recorded in Windows' "Programs and Features" list.
#[cfg(windows)]
fn registry_candidates() -> Vec<PathBuf> {
    use winreg::{enums::*, RegKey};
    const UNINSTALL: &[&str] = &[
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall",
        r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    ];
    let mut found = vec![];
    for hive in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
        for path in UNINSTALL {
            let Ok(root) = RegKey::predef(hive).open_subkey(path) else { continue };
            for app in root.enum_keys().flatten().filter_map(|k| root.open_subkey(k).ok()) {
                let name: String = app.get_value("DisplayName").unwrap_or_default();
                if !name.to_lowercase().contains("tabula rasa") {
                    continue;
                }
                if let Ok(loc) = app.get_value::<String, _>("InstallLocation") {
                    found.push(PathBuf::from(loc.trim_matches('"')));
                }
            }
        }
    }
    found
}

#[cfg(not(windows))]
fn registry_candidates() -> Vec<PathBuf> {
    vec![]
}

/// Default folders the NCsoft installer (or players) commonly used, on every drive.
fn common_candidates() -> Vec<PathBuf> {
    const SUBDIRS: &[&str] = &[
        r"Program Files (x86)\NCsoft\Tabula Rasa",
        r"Program Files\NCsoft\Tabula Rasa",
        r"NCsoft\Tabula Rasa",
        r"Games\Tabula Rasa",
        r"Tabula Rasa",
    ];
    ('C'..='Z')
        .map(|d| PathBuf::from(format!(r"{d}:\")))
        .filter(|root| root.exists())
        .flat_map(|root| SUBDIRS.iter().map(move |s| root.join(s)))
        .collect()
}

fn detect_game_dir() -> Option<PathBuf> {
    registry_candidates()
        .into_iter()
        .chain(common_candidates())
        .find(|d| is_game_dir(d))
}

/// Returns the remembered game folder if it still contains the client,
/// otherwise tries to auto-detect an install and remembers it.
#[tauri::command]
fn get_game_dir(app: AppHandle) -> Option<PathBuf> {
    if let Some(dir) = load_settings(&app).game_dir.filter(|d| is_game_dir(d)) {
        return Some(dir);
    }
    let dir = detect_game_dir()?;
    let _ = save_settings(&app, &Settings { game_dir: Some(dir.clone()) });
    Some(dir)
}

#[tauri::command]
fn set_game_dir(app: AppHandle, dir: PathBuf) -> Result<(), String> {
    if !is_game_dir(&dir) {
        return Err(format!("{GAME_EXE} was not found in that folder."));
    }
    save_settings(&app, &Settings { game_dir: Some(dir) })
}

/// Compares the install against the manifest. `full` hashes every file;
/// otherwise only presence and size are checked.
#[tauri::command]
async fn verify_files(app: AppHandle, dir: PathBuf, full: bool) -> Result<VerifyResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let manifest = manifest()?;
        let total = manifest.files.len();
        let mut result = VerifyResult {
            version: manifest.version,
            checked: total,
            missing: vec![],
            corrupt: vec![],
        };

        for (i, entry) in manifest.files.iter().enumerate() {
            let _ = app.emit("verify-progress", Progress { done: i, total });
            let path = dir.join(&entry.path);
            let Ok(meta) = fs::metadata(&path) else {
                result.missing.push(entry.path.clone());
                continue;
            };
            let ok = meta.len() == entry.size
                && (!full || sha256_file(&path).is_ok_and(|h| h == entry.sha256));
            if !ok {
                result.corrupt.push(entry.path.clone());
            }
        }

        let _ = app.emit("verify-progress", Progress { done: total, total });
        Ok(result)
    })
    .await
    .map_err(|e| e.to_string())?
}

const CLIENT_CFG: &str = "client.cfg";
const INTRO_KEY: &str = "Client.UserInterface.ShowIntroMovie";

fn cfg_key_of(line: &str) -> Option<&str> {
    line.split_once('=').map(|(k, _)| k.trim())
}

/// True if client.cfg has the intro movie turned off. A missing key means the game default (intro on).
#[tauri::command]
fn get_skip_intro(dir: PathBuf) -> bool {
    fs::read_to_string(dir.join(CLIENT_CFG))
        .ok()
        .and_then(|cfg| {
            cfg.lines()
                .find(|l| cfg_key_of(l) == Some(INTRO_KEY))
                .map(|l| l.split_once('=').unwrap().1.trim().eq_ignore_ascii_case("false"))
        })
        .unwrap_or(false)
}

/// Sets ShowIntroMovie in client.cfg, leaving every other line untouched.
fn write_skip_intro(dir: &Path, skip: bool) -> Result<(), String> {
    let path = dir.join(CLIENT_CFG);
    let line = format!("{INTRO_KEY} = {}", if skip { "False" } else { "True" });
    let cfg = fs::read_to_string(&path).unwrap_or_default();
    let nl = if cfg.contains("\r\n") || cfg.is_empty() { "\r\n" } else { "\n" };

    let mut lines: Vec<String> = cfg.lines().map(str::to_owned).collect();
    if let Some(existing) = lines.iter_mut().find(|l| cfg_key_of(l) == Some(INTRO_KEY)) {
        *existing = line;
    } else if let Some(i) = lines.iter().position(|l| l.trim() == "[Options]") {
        lines.insert(i + 1, line);
    } else {
        // Missing keys fall back to game defaults, so a minimal file is valid.
        if lines.is_empty() {
            lines.extend(["[ConfigInfo]".into(), "Version = 2".into()]);
        }
        lines.extend([String::new(), "[Options]".into(), line]);
    }

    fs::write(&path, lines.join(nl) + nl).map_err(|e| format!("Could not update {CLIENT_CFG}: {e}"))
}

#[tauri::command]
fn launch_game(dir: PathBuf, skip_intro: bool) -> Result<(), String> {
    // Best effort: installs under Program Files may be read-only; never block launching.
    let _ = write_skip_intro(&dir, skip_intro);
    Command::new(dir.join(GAME_EXE))
        .current_dir(&dir)
        .args(["/NoPatch", &format!("/AuthServer={AUTH_SERVER}")])
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to start the game: {e}"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_game_dir,
            set_game_dir,
            verify_files,
            get_skip_intro,
            launch_game
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
