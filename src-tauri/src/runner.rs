//! Starting the Windows client outside Windows: through Wine, or through an
//! installed Lutris game entry.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const LUTRIS_FLATPAK: &str = "net.lutris.Lutris";
/// The Lutris .yml as it was before the launcher first rewrote it.
const BACKUP_SUFFIX: &str = ".irl-backup";

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Wine,
    Lutris,
}

/// An installed Lutris game, as recorded in Lutris' pga.db.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LutrisGame {
    pub id: i64,
    pub name: String,
    /// The game's .yml config, pointed at the client before each launch.
    pub config: PathBuf,
    /// Lutris is the Flatpak (net.lutris.Lutris) rather than a native install.
    pub flatpak: bool,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct LaunchConfig {
    pub mode: Mode,
    /// Wine binary: "wine" from PATH, or the full path of another Wine build.
    pub wine: String,
    /// WINEPREFIX for Wine mode; empty means Wine's default (~/.wine).
    pub wine_prefix: String,
    pub lutris_game: Option<LutrisGame>,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            mode: Mode::Wine,
            wine: "wine".into(),
            wine_prefix: String::new(),
            lutris_game: None,
        }
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn xdg_dir(var: &str, fallback: &str) -> PathBuf {
    std::env::var_os(var)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home().join(fallback))
}

struct LutrisInstall {
    /// Folder holding pga.db.
    data: PathBuf,
    /// Folders game .yml files live in (current layout first, then the older one).
    configs: [PathBuf; 2],
    flatpak: bool,
}

fn lutris_installs() -> [LutrisInstall; 2] {
    let flatpak = home().join(".var/app").join(LUTRIS_FLATPAK);
    let native = xdg_dir("XDG_DATA_HOME", ".local/share").join("lutris");
    [
        LutrisInstall {
            data: flatpak.join("data/lutris"),
            configs: [flatpak.join("data/lutris/games"), flatpak.join("config/lutris/games")],
            flatpak: true,
        },
        LutrisInstall {
            configs: [native.join("games"), xdg_dir("XDG_CONFIG_HOME", ".config").join("lutris/games")],
            data: native,
            flatpak: false,
        },
    ]
}

/// Installed Lutris games (native and Flatpak) that have a config file.
pub fn lutris_games() -> Result<Vec<LutrisGame>, String> {
    use rusqlite::{Connection, OpenFlags};

    let mut games = vec![];
    let mut found = false;
    for install in lutris_installs() {
        let db = install.data.join("pga.db");
        if !db.is_file() {
            continue;
        }
        found = true;
        let read = || -> rusqlite::Result<Vec<(i64, String, String)>> {
            let conn = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            let mut stmt = conn.prepare(
                "SELECT id, name, configpath FROM games \
                 WHERE installed = 1 AND configpath <> '' ORDER BY name COLLATE NOCASE",
            )?;
            // Rows with NULL columns are skipped.
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            Ok(rows.flatten().collect())
        };
        let rows = read().map_err(|e| format!("Could not read {}: {e}", db.display()))?;
        for (id, name, configpath) in rows {
            let file = format!("{configpath}.yml");
            if let Some(config) = install.configs.iter().map(|d| d.join(&file)).find(|p| p.is_file()) {
                games.push(LutrisGame { id, name, config, flatpak: install.flatpak });
            }
        }
    }
    if !found {
        return Err("Lutris was not found (no pga.db in its native or Flatpak data folder).".into());
    }
    Ok(games)
}

/// Folders a Linux install of the client is commonly in: the exe of any Lutris game,
/// the folders in ~/Games, and the default Wine prefix.
pub fn game_dir_candidates() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = lutris_games()
        .unwrap_or_default()
        .iter()
        .filter_map(|g| fs::read_to_string(&g.config).ok())
        .filter_map(|yml| game_value(&yml, "exe"))
        .filter_map(|exe| Path::new(&exe).parent().map(Path::to_path_buf))
        .collect();
    if let Ok(entries) = fs::read_dir(home().join("Games")) {
        dirs.extend(entries.flatten().map(|e| e.path()));
    }
    let drive_c = home().join(".wine/drive_c");
    dirs.extend(
        ["Program Files (x86)/NCsoft/Tabula Rasa", "Program Files/NCsoft/Tabula Rasa"].map(|s| drive_c.join(s)),
    );
    dirs
}

/// Starts the client in `dir` with `args` (the arguments Windows passes it).
pub fn launch(cfg: &LaunchConfig, dir: &Path, args: &[String]) -> Result<(), String> {
    let exe = dir.join(crate::game::GAME_EXE);
    match cfg.mode {
        Mode::Wine => {
            let wine = match cfg.wine.trim() {
                "" => "wine",
                w => w,
            };
            let mut cmd = Command::new(wine);
            cmd.arg(&exe).args(args).current_dir(dir);
            if !cfg.wine_prefix.trim().is_empty() {
                cmd.env("WINEPREFIX", cfg.wine_prefix.trim());
            }
            spawn(cmd, wine)
        }
        Mode::Lutris => {
            let game = cfg.lutris_game.as_ref().ok_or("Choose a Lutris game in Settings.")?;
            // Lutris has no way to pass arguments at launch, so they go into the game's .yml.
            // A saved password is left out: it would sit in that file in plain text.
            let args: Vec<&str> =
                args.iter().map(String::as_str).filter(|a| !a.starts_with("/password=")).collect();
            point_lutris_at(&game.config, &exe, &args, dir)?;

            let uri = format!("lutris:rungameid/{}", game.id);
            let mut cmd = Command::new(if game.flatpak { "flatpak" } else { "lutris" });
            if game.flatpak {
                cmd.args(["run", LUTRIS_FLATPAK]);
            }
            cmd.arg(uri);
            spawn(cmd, "Lutris")
        }
    }
}

fn spawn(mut cmd: Command, what: &str) -> Result<(), String> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Failed to start {what}: {e}"))?;
    // Reap it when it exits so it does not linger as a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Sets exe, args and working_dir of a Lutris game config, keeping a one-time backup.
fn point_lutris_at(config: &Path, exe: &Path, args: &[&str], dir: &Path) -> Result<(), String> {
    let utf8 = |p: &Path| p.to_str().map(str::to_owned).ok_or_else(|| format!("Path is not valid UTF-8: {}", p.display()));
    let yml = fs::read_to_string(config).map_err(|e| format!("Could not read {}: {e}", config.display()))?;

    let mut backup = config.as_os_str().to_owned();
    backup.push(BACKUP_SUFFIX);
    let backup = PathBuf::from(backup);
    if !backup.exists() {
        fs::copy(config, &backup).map_err(|e| format!("Could not back up {}: {e}", config.display()))?;
    }

    let args = args.iter().map(|a| shell_quote(a)).collect::<Vec<_>>().join(" ");
    let updated = set_game_values(&yml, &[("exe", utf8(exe)?), ("args", args), ("working_dir", utf8(dir)?)]);

    let mut tmp = config.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, updated)
        .and_then(|()| fs::rename(&tmp, config))
        .map_err(|e| format!("Could not update {}: {e}", config.display()))
}

// ---------- minimal YAML handling for the top-level "game:" block ----------

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Indent of the game block's keys: that of its first line (Lutris writes 2, users may not).
fn child_indent<S: AsRef<str>>(lines: &[S], game: usize) -> usize {
    lines[game + 1..]
        .iter()
        .map(AsRef::as_ref)
        .find(|l| !l.trim().is_empty())
        .map(indent_of)
        .filter(|&n| n > 0)
        .unwrap_or(2)
}

/// True while `line` is still inside a top-level block (indented or blank).
fn in_block(line: &str) -> bool {
    line.trim().is_empty() || line.starts_with(char::is_whitespace)
}

fn yaml_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn yaml_unquote(value: &str) -> String {
    let inner = |q: char| value.len() >= 2 && value.starts_with(q) && value.ends_with(q);
    if inner('\'') {
        value[1..value.len() - 1].replace("''", "'")
    } else if inner('"') {
        value[1..value.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\")
    } else {
        value.to_owned()
    }
}

/// Quotes one argument for Lutris, which splits `args` the way a POSIX shell does.
fn shell_quote(arg: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/=:.,_-+@%".contains(c);
    if !arg.is_empty() && arg.chars().all(plain) {
        arg.to_owned()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

/// A single-line value from the "game:" block.
fn game_value(yml: &str, key: &str) -> Option<String> {
    let lines: Vec<&str> = yml.lines().collect();
    let game = lines.iter().position(|l| l.trim_end() == "game:")?;
    let indent = child_indent(&lines, game);
    let prefix = format!("{key}:");
    lines[game + 1..]
        .iter()
        .take_while(|l| in_block(l))
        .find(|l| indent_of(l) == indent && l.trim_start().starts_with(&prefix))
        .map(|l| yaml_unquote(l.trim_start()[prefix.len()..].trim()))
}

/// Replaces `values` in the "game:" block, leaving every other line untouched.
/// Only keys at the block's own indent count, so nested ones (e.g. in launch_configs) stay.
fn set_game_values(yml: &str, values: &[(&str, String)]) -> String {
    let mut lines: Vec<String> = yml.lines().map(str::to_owned).collect();
    let game = lines.iter().position(|l| l.trim_end() == "game:").unwrap_or_else(|| {
        lines.push("game:".into());
        lines.len() - 1
    });
    let indent = child_indent(&lines, game);

    let mut i = game + 1;
    while i < lines.len() && in_block(&lines[i]) {
        let line = &lines[i];
        let owned = !line.trim().is_empty()
            && indent_of(line) == indent
            && values.iter().any(|(k, _)| line.trim_start().starts_with(&format!("{k}:")));
        if !owned {
            i += 1;
            continue;
        }
        // Continuation lines are deeper; blank lines inside a block scalar belong to it too.
        let mut last = i;
        for (j, l) in lines.iter().enumerate().skip(i + 1) {
            if !l.trim().is_empty() && indent_of(l) <= indent {
                break;
            }
            if !l.trim().is_empty() {
                last = j;
            }
        }
        lines.drain(i..=last);
    }

    let pad = " ".repeat(indent);
    for (n, (key, value)) in values.iter().enumerate() {
        lines.insert(game + 1 + n, format!("{pad}{key}: {}", yaml_quote(value)));
    }
    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> Vec<(&'static str, String)> {
        vec![
            ("exe", "/g/Tabula Rasa/tabula_rasa.exe".into()),
            ("args", "/NoPatch /AuthServer=h:2106".into()),
            ("working_dir", "/g/Tabula Rasa".into()),
        ]
    }

    #[test]
    fn replaces_game_keys_and_keeps_the_rest() {
        let yml = "game:\n  args: old\n  exe: old.exe\n  prefix: /p\n  launch_configs:\n  - exe: other.exe\n    args: x\n  working_dir: |\n    /a\n\n    /b\nwine:\n  version: ge\n";
        let out = set_game_values(yml, &values());
        assert_eq!(
            out,
            "game:\n  exe: '/g/Tabula Rasa/tabula_rasa.exe'\n  args: '/NoPatch /AuthServer=h:2106'\n  working_dir: '/g/Tabula Rasa'\n  prefix: /p\n  launch_configs:\n  - exe: other.exe\n    args: x\nwine:\n  version: ge\n"
        );
    }

    #[test]
    fn adds_a_game_block_when_missing() {
        let out = set_game_values("wine:\n  version: ge\n", &values());
        assert!(out.starts_with("wine:\n  version: ge\ngame:\n  exe: '/g/Tabula Rasa/tabula_rasa.exe'\n"));
    }

    #[test]
    fn follows_the_block_indent() {
        let out = set_game_values("game:\n    exe: a\n    prefix: /p\n", &values());
        assert_eq!(out.lines().nth(1), Some("    exe: '/g/Tabula Rasa/tabula_rasa.exe'"));
        assert!(out.contains("\n    prefix: /p\n"));
    }

    #[test]
    fn rewrite_is_stable_and_readable() {
        let once = set_game_values("game:\n  exe: a\n", &values());
        assert_eq!(set_game_values(&once, &values()), once);
        assert_eq!(game_value(&once, "exe").as_deref(), Some("/g/Tabula Rasa/tabula_rasa.exe"));
        assert_eq!(game_value("game:\n  exe: \"/x/it's.exe\"\n", "exe").as_deref(), Some("/x/it's.exe"));
        assert_eq!(game_value("game:\n  exe: '/x/it''s.exe'\n", "exe").as_deref(), Some("/x/it's.exe"));
        assert_eq!(game_value("wine:\n  exe: a\n", "exe"), None);
    }

    #[test]
    fn quotes_args_for_lutris() {
        assert_eq!(shell_quote("/AuthServer=game.infiniterasa.org:2106"), "/AuthServer=game.infiniterasa.org:2106");
        assert_eq!(shell_quote("/user=Joe Bloggs"), "'/user=Joe Bloggs'");
        assert_eq!(shell_quote("/user=o'neil"), r"'/user=o'\''neil'");
    }
}
