mod game;
mod news;
#[cfg(not(windows))]
mod runner;
mod settings;

use serde::Serialize;
use settings::Settings;
use std::path::PathBuf;
#[cfg(windows)]
use std::process::Command;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone)]
struct Progress {
    done: usize,
    total: usize,
}

/// Settings, with the game folder auto-detected (and remembered) when unset or no longer valid.
#[tauri::command]
fn get_settings(app: AppHandle) -> Settings {
    let mut s = settings::load(&app);
    if !s.game_dir.as_deref().is_some_and(game::is_game_dir) {
        s.game_dir = game::detect_game_dir();
        if s.game_dir.is_some() {
            let _ = settings::save(&app, &s);
        }
    }
    s
}

#[tauri::command]
fn save_settings(app: AppHandle, mut settings: Settings) -> Result<(), String> {
    // Only verify_files decides what has been verified.
    settings.verified_dir = settings::load(&app).verified_dir;
    settings::save(&app, &settings)
}

#[tauri::command]
fn is_game_dir(dir: PathBuf) -> bool {
    game::is_game_dir(&dir)
}

#[tauri::command]
fn has_password(server_id: String) -> bool {
    settings::password(&server_id).is_some()
}

#[tauri::command]
fn set_password(server_id: String, password: Option<String>) -> Result<(), String> {
    settings::set_password(&server_id, password.as_deref())
}

/// Whether passwords can be saved; outside Windows a desktop keyring may be missing.
#[cfg(not(windows))]
#[tauri::command]
async fn can_save_passwords() -> bool {
    tauri::async_runtime::spawn_blocking(settings::can_save_passwords).await.unwrap_or(false)
}

/// Checks the install. A clean full verify is remembered so later starts only need the quick check.
#[tauri::command]
async fn verify_files(app: AppHandle, dir: PathBuf, full: bool) -> Result<game::VerifyResult, String> {
    let result = tauri::async_runtime::spawn_blocking({
        let (app, dir) = (app.clone(), dir.clone());
        move || {
            game::verify(&dir, full, |done, total| {
                let _ = app.emit("verify-progress", Progress { done, total });
            })
        }
    })
    .await
    .map_err(|e| e.to_string())??;

    let clean = result.missing.is_empty() && result.corrupt.is_empty();
    let mut s = settings::load(&app);
    if full && clean {
        s.verified_dir = Some(dir);
    } else if !clean && s.verified_dir.as_ref() == Some(&dir) {
        s.verified_dir = None;
    }
    settings::save(&app, &s)?;
    Ok(result)
}

#[tauri::command]
fn get_skip_intro(dir: PathBuf) -> bool {
    game::skip_intro(&dir)
}

#[tauri::command]
fn set_skip_intro(dir: PathBuf, skip: bool) -> Result<(), String> {
    game::write_skip_intro(&dir, skip)
}

#[tauri::command]
async fn get_news() -> Result<Vec<news::NewsItem>, String> {
    news::fetch().await
}

/// Starts the client against the selected server, logging in automatically
/// when a username (and optionally a remembered password) is saved for it.
#[tauri::command]
fn launch_game(app: AppHandle) -> Result<(), String> {
    let s = settings::load(&app);
    let dir = s.game_dir.filter(|d| game::is_game_dir(d)).ok_or("Game folder is not set.")?;
    let server = s
        .servers
        .iter()
        .find(|sv| sv.id == s.selected_server)
        .ok_or("Select a server first.")?;

    let mut args = vec!["/NoPatch".to_owned(), format!("/AuthServer={}", server.host)];
    if !server.username.is_empty() {
        args.push(format!("/user={}", server.username));
        if let Some(password) = settings::password(&server.id) {
            args.push(format!("/password={password}"));
        }
    }

    #[cfg(windows)]
    Command::new(dir.join(game::GAME_EXE))
        .current_dir(&dir)
        .args(&args)
        .spawn()
        .map_err(|e| format!("Failed to start the game: {e}"))?;
    #[cfg(not(windows))]
    runner::launch(&s.launch, &dir, &args)?;

    if s.close_on_launch {
        app.exit(0);
    }
    Ok(())
}

/// Installed Lutris games the client can be launched through.
#[cfg(not(windows))]
#[tauri::command]
async fn list_lutris_games() -> Result<Vec<runner::LutrisGame>, String> {
    tauri::async_runtime::spawn_blocking(runner::lutris_games).await.map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            is_game_dir,
            has_password,
            set_password,
            #[cfg(not(windows))]
            can_save_passwords,
            verify_files,
            get_skip_intro,
            set_skip_intro,
            get_news,
            launch_game,
            #[cfg(not(windows))]
            list_lutris_games
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
