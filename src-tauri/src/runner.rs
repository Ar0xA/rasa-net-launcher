//! Starting the Windows client outside Windows: through Wine, or through an
//! installed Lutris game entry.

use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

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
            data: native.clone(),
            configs: [native.join("games"), xdg_dir("XDG_CONFIG_HOME", ".config").join("lutris/games")],
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
            // Lutris may be writing to it (e.g. lastplayed when a game starts).
            conn.busy_timeout(Duration::from_millis(500))?;
            let mut stmt = conn.prepare(
                "SELECT id, name, configpath FROM games \
                 WHERE installed = 1 AND name IS NOT NULL AND configpath <> '' ORDER BY name COLLATE NOCASE",
            )?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect()
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
/// The output of Wine (or Lutris) goes to `log` when given.
pub fn launch(cfg: &LaunchConfig, dir: &Path, args: &[String], log: Option<&Path>) -> Result<(), String> {
    let exe = crate::game::resolve(dir, crate::game::GAME_EXE);
    match cfg.mode {
        Mode::Wine => {
            let wine = match cfg.wine.trim() {
                "" => "wine",
                w => w,
            };
            let mut cmd = Command::new(wine);
            cmd.arg(&exe).args(args).current_dir(dir);
            let prefix = match cfg.wine_prefix.trim() {
                "" => prefix_of(dir),
                p => Some(PathBuf::from(p)),
            };
            if let Some(prefix) = prefix {
                cmd.env("WINEPREFIX", prefix);
            }
            spawn(cmd, wine, log)
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
            spawn(cmd, "Lutris", log)
        }
    }
}

/// The Wine prefix a folder is in: the nearest ancestor holding drive_c.
fn prefix_of(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().find(|a| a.join("drive_c").is_dir()).map(Path::to_path_buf)
}

/// Wine builds on this machine: the one on PATH, then Lutris' runners (native and
/// Flatpak), newest first.
pub fn wine_runners() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::env::var_os("PATH")
        .iter()
        .flat_map(std::env::split_paths)
        .map(|d| d.join("wine"))
        .filter(|w| w.is_file())
        .take(1)
        .collect();
    for install in lutris_installs() {
        let mut runners: Vec<PathBuf> = fs::read_dir(install.data.join("runners/wine"))
            .map(|e| e.flatten().map(|e| e.path().join("bin/wine")).filter(|w| w.is_file()).collect())
            .unwrap_or_default();
        runners.sort();
        found.extend(runners.into_iter().rev());
    }
    found
}

/// Sends the child's output to `log`, or discards it.
fn output(log: Option<&Path>) -> (Stdio, Stdio) {
    let file = log.and_then(|p| {
        fs::create_dir_all(p.parent()?).ok()?;
        let out = fs::File::create(p).ok()?;
        Some((out.try_clone().ok()?, out))
    });
    match file {
        Some((out, err)) => (out.into(), err.into()),
        None => (Stdio::null(), Stdio::null()),
    }
}

fn spawn(mut cmd: Command, what: &str, log: Option<&Path>) -> Result<(), String> {
    for (key, value) in host_env(std::env::vars_os()) {
        match value {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        };
    }
    let (stdout, stderr) = output(log);
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .map_err(|e| format!("Failed to start {what}: {e}"))?;
    // Reap it when it exits so it does not linger as a zombie.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The environment changes that give a started program the host's environment back when
/// the launcher runs as an AppImage. AppRun points LD_LIBRARY_PATH, PYTHONHOME, GTK_PATH
/// and others into the AppImage's mount for its own bundled libraries; passed on, host
/// programs load those instead of their own: flatpak fails at once ("symbol lookup error"
/// in libcurl) and a native Lutris cannot start Python. Entries inside the mount are
/// dropped from path lists, a variable left empty is removed, and so are the AppImage's
/// own variables; anything else is kept as it was.
fn host_env<I>(vars: I) -> Vec<(OsString, Option<OsString>)>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    let vars: Vec<_> = vars.into_iter().collect();
    let value_of = |name: &str| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v);
    let appdir = match (value_of("APPDIR"), value_of("APPIMAGE")) {
        (Some(dir), Some(_)) => dir.to_string_lossy().trim_end_matches('/').to_owned(),
        _ => return Vec::new(),
    };
    if appdir.is_empty() {
        return Vec::new();
    }
    let inside = |part: &str| part == appdir || part.starts_with(&format!("{appdir}/"));

    let mut changes = Vec::new();
    for (key, value) in vars {
        if key == "APPDIR" || key == "APPIMAGE" || key == "ARGV0" || key == "OWD" {
            changes.push((key, None));
            continue;
        }
        let Some(text) = value.to_str() else { continue };
        if !text.split(':').any(inside) {
            continue;
        }
        let kept: Vec<&str> = text.split(':').filter(|p| !p.is_empty() && !inside(p)).collect();
        changes.push((key, if kept.is_empty() { None } else { Some(kept.join(":").into()) }));
    }
    changes
}

/// Sets exe, args and working_dir of a Lutris game config, keeping a one-time backup.
fn point_lutris_at(config: &Path, exe: &Path, args: &[&str], dir: &Path) -> Result<(), String> {
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

fn utf8(path: &Path) -> Result<String, String> {
    path.to_str().map(str::to_owned).ok_or_else(|| format!("Path is not valid UTF-8: {}", path.display()))
}

// ---------- minimal YAML handling for the top-level "game:" block ----------

fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

/// The top-level "game:" line, also with a trailing comment or as an empty `game: {}`.
fn is_game_line(line: &str) -> bool {
    let line = line.split_once(" #").map_or(line, |(l, _)| l).trim_end();
    line == "game:" || line == "game: {}"
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// Indent of the game block's keys: that of its first line (Lutris writes 2, users may not).
fn child_indent<S: AsRef<str>>(lines: &[S], game: usize) -> usize {
    lines[game + 1..]
        .iter()
        .map(AsRef::as_ref)
        .find(|l| !l.trim().is_empty() && !is_comment(l))
        .map(indent_of)
        .filter(|&n| n > 0)
        .unwrap_or(2)
}

/// True while `line` is still inside a top-level block (indented, blank or a comment).
fn in_block(line: &str) -> bool {
    line.trim().is_empty() || line.starts_with(char::is_whitespace) || is_comment(line)
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
    let game = lines.iter().position(|l| is_game_line(l))?;
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
    let game = lines.iter().position(|l| is_game_line(l)).unwrap_or_else(|| {
        lines.push("game:".into());
        lines.len() - 1
    });
    if lines[game].contains("{}") {
        lines[game] = "game:".into(); // keys follow on their own lines
    }
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
        // A comment at the key's indent or less ends the value and is kept.
        let mut last = i;
        for (j, l) in lines.iter().enumerate().skip(i + 1) {
            let blank = l.trim().is_empty();
            if !blank && indent_of(l) <= indent {
                break;
            }
            if !blank {
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

    fn env(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs.iter().map(|(k, v)| (k.into(), v.into())).collect()
    }

    fn change<'a>(changes: &'a [(OsString, Option<OsString>)], key: &str) -> Option<&'a Option<OsString>> {
        changes.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    #[test]
    fn host_env_changes_nothing_outside_an_appimage() {
        let vars = env(&[("LD_LIBRARY_PATH", "/opt/lib"), ("PATH", "/usr/bin")]);
        assert!(host_env(vars).is_empty());
        // APPDIR alone (no AppImage runtime) or a root APPDIR is not trusted either.
        assert!(host_env(env(&[("APPDIR", "/tmp/x"), ("PATH", "/tmp/x/bin")])).is_empty());
        assert!(host_env(env(&[("APPDIR", "/"), ("APPIMAGE", "/a"), ("PATH", "/usr/bin")])).is_empty());
    }

    #[test]
    fn host_env_drops_the_appimage_mount_and_keeps_the_rest() {
        let m = "/tmp/.mount_rasa-abc";
        let vars = env(&[
            ("APPDIR", m),
            ("APPIMAGE", "/home/u/rasa-launcher"),
            ("LD_LIBRARY_PATH", &format!("{m}/usr/lib/:{m}/usr/lib/x86_64-linux-gnu/")),
            ("PYTHONHOME", &format!("{m}/usr/")),
            ("PYTHONPATH", &format!("{m}/usr/share/pyshared/:")),
            ("GTK_PATH", &format!("{m}//usr/lib/gtk-3.0")),
            ("PATH", &format!("{m}/usr/bin/:/usr/local/bin:/usr/bin")),
            ("XDG_DATA_DIRS", &format!("{m}/usr/share/:/usr/share:/var/lib/flatpak/exports/share")),
            ("HOME", "/home/u"),
            ("PYTHONDONTWRITEBYTECODE", "1"),
            ("OTHER", "/tmp/.mount_rasa-abcdef/x"),
        ]);
        let changes = host_env(vars);

        assert_eq!(change(&changes, "LD_LIBRARY_PATH"), Some(&None));
        assert_eq!(change(&changes, "PYTHONHOME"), Some(&None));
        assert_eq!(change(&changes, "PYTHONPATH"), Some(&None));
        assert_eq!(change(&changes, "GTK_PATH"), Some(&None));
        assert_eq!(change(&changes, "APPDIR"), Some(&None));
        assert_eq!(change(&changes, "APPIMAGE"), Some(&None));
        assert_eq!(change(&changes, "PATH"), Some(&Some("/usr/local/bin:/usr/bin".into())));
        assert_eq!(
            change(&changes, "XDG_DATA_DIRS"),
            Some(&Some("/usr/share:/var/lib/flatpak/exports/share".into()))
        );
        assert_eq!(change(&changes, "HOME"), None);
        assert_eq!(change(&changes, "PYTHONDONTWRITEBYTECODE"), None);
        // A different mount that only shares a prefix is not the AppImage's.
        assert_eq!(change(&changes, "OTHER"), None);
    }

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
    fn handles_comments_and_empty_game_blocks() {
        let yml = "game: # main\n  exe: old.exe\n# note\n  args: old\n  prefix: /p\nwine:\n  version: ge\n";
        let out = set_game_values(yml, &values());
        assert_eq!(
            out,
            "game: # main\n  exe: '/g/Tabula Rasa/tabula_rasa.exe'\n  args: '/NoPatch /AuthServer=h:2106'\n  working_dir: '/g/Tabula Rasa'\n# note\n  prefix: /p\nwine:\n  version: ge\n"
        );
        assert_eq!(game_value(yml, "args").as_deref(), Some("old"));

        let out = set_game_values("game: {}\nwine: {}\n", &values());
        assert!(out.starts_with("game:\n  exe: '/g/Tabula Rasa/tabula_rasa.exe'\n"));
        assert!(out.ends_with("'/g/Tabula Rasa'\nwine: {}\n"));
    }

    #[test]
    fn finds_the_prefix_above_the_game() {
        let root = std::env::temp_dir().join(format!("irl-prefix-{}", std::process::id()));
        let game = root.join("drive_c/Program Files/NCsoft/Tabula Rasa");
        fs::create_dir_all(&game).unwrap();
        assert_eq!(prefix_of(&game), Some(root.clone()));
        assert_eq!(prefix_of(&std::env::temp_dir()), None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn quotes_args_for_lutris() {
        assert_eq!(shell_quote("/AuthServer=game.infiniterasa.org:2106"), "/AuthServer=game.infiniterasa.org:2106");
        assert_eq!(shell_quote("/user=Joe Bloggs"), "'/user=Joe Bloggs'");
        assert_eq!(shell_quote("/user=o'neil"), r"'/user=o'\''neil'");
    }
}
