//! The slow mouse look fix by thatawesomeguy (Infinite Rasa Discord): a Python module the client
//! loads from `python_nca_internal/client_nca_internal` in the game folder. In mouse look the client
//! drops cursor moves of 1 UI unit, so slow mouse movement is lost (worst at high resolutions); the
//! module keeps such moves until they add up to more than 1 unit. See
//! `mods/slowmouselook/client_nca_internal/slowmouselook.py`.
//!
//! Install and uninstall leave anything else in that folder alone: another mod's
//! `client_nca_internal/__init__.py` gets our import lines as a marked block (and loses only that
//! block again), and a `developerkeys.py` that is not ours is never written or removed. Files are
//! read and written byte for byte (Latin-1), so a foreign file that is not UTF-8 keeps its bytes.
//! A copy installed by hand from the fix's own zip (its `__init__.py` holds only the import lines)
//! counts as installed and is removed as well.

use crate::game::resolve;
use std::fs;
use std::path::{Path, PathBuf};

const PACKAGE: &str = "python_nca_internal/client_nca_internal";
const MODULE: &str = "slowmouselook";
const MODULE_PY: &str = include_str!("../mods/slowmouselook/client_nca_internal/slowmouselook.py");
const DEVELOPER_KEYS_PY: &str = include_str!("../mods/slowmouselook/client_nca_internal/developerkeys.py");

const BEGIN: &str = "# BEGIN slow mouse look fix (Infinite Rasa launcher)";
const END: &str = "# END slow mouse look fix";
/// The lines that load the module, between BEGIN and END in `__init__.py`.
const IMPORT_LINES: &[&str] = &[
    "# tabula_rasa.exe puts python_nca_internal (in the game folder) first on sys.path; the client imports this package from",
    "# gameui.SetupInputMappings (errors there are swallowed, so a broken mod just does nothing).",
    "try:",
    "    import client_nca_internal.slowmouselook",
    "except Exception, _e:",
    "    from client.log import TR_LOG_ERROR",
    "    TR_LOG_ERROR('slowmouselook failed to load: %s', _e)",
];

fn package(dir: &Path) -> PathBuf {
    resolve(dir, PACKAGE)
}

/// The file and the bytecode Python 2.4 may have written next to it.
fn remove_module(path: &Path) -> Result<(), String> {
    for ext in ["py", "pyc", "pyo"] {
        let p = path.with_extension(ext);
        if p.exists() {
            fs::remove_file(&p).map_err(|e| format!("Could not remove {}: {e}", p.display()))?;
        }
    }
    Ok(())
}

fn same_text(a: &str, b: &str) -> bool {
    a.replace("\r\n", "\n").trim_end() == b.replace("\r\n", "\n").trim_end()
}

/// The file's bytes as Latin-1 text; None when there is no file. Any other read error is an error:
/// treating it as an empty file would overwrite another mod's file.
fn read_text(path: &Path) -> Result<Option<String>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes.iter().map(|&b| b as char).collect())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Could not read {}: {e}", path.display())),
    }
}

/// Writes Latin-1 text from `read_text` (or ASCII) back as the same bytes.
fn write_text(path: &Path, text: &str) -> Result<(), String> {
    let bytes: Vec<u8> = text.chars().map(|c| c as u32 as u8).collect();
    fs::write(path, bytes).map_err(|e| format!("Could not write {}: {e}", path.display()))
}

/// The fix's own `__init__.py` from its zip: only the import lines.
fn is_zip_init(text: &str) -> bool {
    same_text(text, &IMPORT_LINES.join("\n"))
}

/// Whether `__init__.py` loads the fix: our block, or the zip's own file.
fn loads_fix(text: &str) -> bool {
    without_block(text).1 || is_zip_init(text)
}

/// `text` without the BEGIN..END block, and whether there was one.
fn without_block(text: &str) -> (Vec<&str>, bool) {
    let mut kept = vec![];
    let (mut inside, mut found) = (false, false);
    for line in text.lines() {
        if line.trim_end() == BEGIN {
            inside = true;
            found = true;
        } else if inside && line.trim_end() == END {
            inside = false;
        } else if !inside {
            kept.push(line);
        }
    }
    (kept, found)
}

/// True when the fix is in the game folder: the module and its import in `__init__.py`.
pub fn installed(dir: &Path) -> bool {
    let pkg = package(dir);
    pkg.join(format!("{MODULE}.py")).is_file()
        && read_text(&pkg.join("__init__.py")).ok().flatten().is_some_and(|t| loads_fix(&t))
}

pub fn install(dir: &Path) -> Result<(), String> {
    let pkg = package(dir);
    fs::create_dir_all(&pkg).map_err(|e| format!("Could not create {}: {e}", pkg.display()))?;
    let init = pkg.join("__init__.py");
    // Read first: an unreadable foreign __init__.py stops the install before anything is written.
    let mut text = read_text(&init)?.unwrap_or_default();
    if is_zip_init(&text) {
        text.clear(); // a hand install of this fix: replaced by the marked block
    }
    let write = |name: &str, text: &str| write_text(&pkg.join(name), text);

    write(&format!("{MODULE}.py"), MODULE_PY)?;
    // Old bytecode of an earlier version would be loaded first if its time stamp is newer.
    for ext in ["pyc", "pyo"] {
        let _ = fs::remove_file(pkg.join(format!("{MODULE}.{ext}")));
    }
    // The client calls both functions as soon as client_nca_internal exists.
    if !pkg.join("developerkeys.py").exists() {
        write("developerkeys.py", DEVELOPER_KEYS_PY)?;
    }

    let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let (mut lines, _) = without_block(&text);
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let mut out: Vec<&str> = lines;
    if !out.is_empty() {
        out.push("");
    }
    out.push(BEGIN);
    out.extend_from_slice(IMPORT_LINES);
    out.push(END);
    write("__init__.py", &(out.join(nl) + nl))
}

pub fn uninstall(dir: &Path) -> Result<(), String> {
    let pkg = package(dir);
    if !pkg.is_dir() {
        return Ok(());
    }
    let init = pkg.join("__init__.py");
    let keys = pkg.join("developerkeys.py");
    let text = read_text(&init)?;
    let keys_ours = read_text(&keys)?.is_some_and(|t| same_text(&t, DEVELOPER_KEYS_PY));
    remove_module(&pkg.join(format!("{MODULE}.py")))?;

    // Another mod's modules in the package: then __init__.py stays (it makes the folder a package).
    let foreign_modules = fs::read_dir(&pkg).is_ok_and(|entries| {
        entries.flatten().any(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name.ends_with(".py") && name != "__init__.py" && !(name == "developerkeys.py" && keys_ours)
        })
    });

    if let Some(text) = text {
        let (mut lines, found) = without_block(&text);
        // The blank line install put before the block.
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        if is_zip_init(&text) || (found && lines.iter().all(|l| l.trim().is_empty()) && !foreign_modules) {
            remove_module(&init)?;
        } else if found {
            let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
            let kept = if lines.iter().all(|l| l.trim().is_empty()) { String::new() } else { lines.join(nl) + nl };
            write_text(&init, &kept)?;
            // Python rebuilds the bytecode from the changed file.
            for ext in ["pyc", "pyo"] {
                let _ = fs::remove_file(init.with_extension(ext));
            }
        }
    }

    // developerkeys.py only goes when it is ours and the package is gone.
    if keys_ours && !init.exists() {
        remove_module(&keys)?;
    }

    // Empty folders go; a folder with anything else in it stays.
    for folder in [pkg.clone(), pkg.parent().map(Path::to_path_buf).unwrap_or_default()] {
        if fs::read_dir(&folder).is_ok_and(|mut d| d.next().is_none()) {
            let _ = fs::remove_dir(&folder);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("irl-mouselook-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("tabula_rasa.exe"), "").unwrap();
        dir
    }

    #[test]
    fn installs_and_removes_everything_in_a_clean_folder() {
        let dir = game_dir("clean");
        assert!(!installed(&dir));
        install(&dir).unwrap();
        assert!(installed(&dir));
        let pkg = dir.join(PACKAGE);
        assert_eq!(fs::read_to_string(pkg.join("slowmouselook.py")).unwrap(), MODULE_PY);
        assert_eq!(fs::read_to_string(pkg.join("developerkeys.py")).unwrap(), DEVELOPER_KEYS_PY);
        assert!(fs::read_to_string(pkg.join("__init__.py")).unwrap().contains("import client_nca_internal.slowmouselook"));

        install(&dir).unwrap(); // again: still one block
        assert_eq!(fs::read_to_string(pkg.join("__init__.py")).unwrap().matches(BEGIN).count(), 1);

        fs::write(pkg.join("slowmouselook.pyc"), "x").unwrap(); // bytecode the game wrote
        uninstall(&dir).unwrap();
        assert!(!installed(&dir));
        assert!(!dir.join("python_nca_internal").exists());
        assert!(dir.join("tabula_rasa.exe").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn leaves_another_mods_files_alone() {
        let dir = game_dir("shared");
        let pkg = dir.join(PACKAGE);
        fs::create_dir_all(&pkg).unwrap();
        let theirs_init = "import client_nca_internal.othermod\r\n";
        let theirs_keys = "def InsertDeveloperKeyMappings():\r\n    print 'keys'\r\n";
        fs::write(pkg.join("__init__.py"), theirs_init).unwrap();
        fs::write(pkg.join("developerkeys.py"), theirs_keys).unwrap();
        fs::write(dir.join("python_nca_internal/clientbootstrap.py"), "").unwrap();

        install(&dir).unwrap();
        assert!(installed(&dir));
        let init = fs::read_to_string(pkg.join("__init__.py")).unwrap();
        assert!(init.starts_with("import client_nca_internal.othermod\r\n"));
        assert!(init.contains("\r\n    import client_nca_internal.slowmouselook\r\n"));
        assert_eq!(fs::read_to_string(pkg.join("developerkeys.py")).unwrap(), theirs_keys);

        uninstall(&dir).unwrap();
        assert!(!installed(&dir));
        assert_eq!(fs::read_to_string(pkg.join("__init__.py")).unwrap(), theirs_init);
        assert_eq!(fs::read_to_string(pkg.join("developerkeys.py")).unwrap(), theirs_keys);
        assert!(!pkg.join("slowmouselook.py").exists());
        assert!(dir.join("python_nca_internal/clientbootstrap.py").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn keeps_an_empty_foreign_init_and_non_utf8_bytes() {
        let dir = game_dir("foreign");
        let pkg = dir.join(PACKAGE);
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("__init__.py"), "").unwrap();
        fs::write(pkg.join("othermod.py"), "x = 1\n").unwrap();
        install(&dir).unwrap();
        uninstall(&dir).unwrap();
        assert_eq!(fs::read(pkg.join("__init__.py")).unwrap(), b"");
        assert!(pkg.join("othermod.py").exists());

        let latin1: &[u8] = b"# caf\xe9 (cp1252)\r\nimport client_nca_internal.othermod\r\n";
        fs::write(pkg.join("__init__.py"), latin1).unwrap();
        install(&dir).unwrap();
        assert!(installed(&dir));
        assert!(fs::read(pkg.join("__init__.py")).unwrap().starts_with(latin1));
        uninstall(&dir).unwrap();
        assert_eq!(fs::read(pkg.join("__init__.py")).unwrap(), latin1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn takes_over_and_removes_a_hand_install_from_the_zip() {
        let dir = game_dir("zip");
        let pkg = dir.join(PACKAGE);
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("__init__.py"), IMPORT_LINES.join("\r\n") + "\r\n").unwrap();
        fs::write(pkg.join("developerkeys.py"), DEVELOPER_KEYS_PY.replace("\r\n", "\n").replace('\n', "\r\n")).unwrap();
        fs::write(pkg.join("slowmouselook.py"), MODULE_PY).unwrap();
        fs::write(pkg.join("slowmouselook.pyo"), "x").unwrap();
        assert!(installed(&dir));

        install(&dir).unwrap(); // the zip's file becomes the marked block, not a second import
        let init = fs::read_to_string(pkg.join("__init__.py")).unwrap();
        assert_eq!(init.matches("import client_nca_internal.slowmouselook").count(), 1);
        assert!(init.contains(BEGIN));

        uninstall(&dir).unwrap();
        assert!(!dir.join("python_nca_internal").exists());

        fs::create_dir_all(&pkg).unwrap(); // and straight from the zip, never touched by install
        fs::write(pkg.join("__init__.py"), IMPORT_LINES.join("\n")).unwrap();
        fs::write(pkg.join("developerkeys.py"), DEVELOPER_KEYS_PY).unwrap();
        fs::write(pkg.join("slowmouselook.py"), MODULE_PY).unwrap();
        uninstall(&dir).unwrap();
        assert!(!dir.join("python_nca_internal").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn keeps_the_infinite_rasa_client_mod_side_load() {
        // That mod's side-load puts its own files straight into python_nca_internal (no package).
        let dir = game_dir("sideload");
        let nca = dir.join("python_nca_internal");
        fs::create_dir_all(&nca).unwrap();
        let theirs = ["clientbootstrap.py", "clientmod_loader.py", "clientmod_data.py"];
        for name in theirs {
            fs::write(nca.join(name), format!("# {name}\r\n")).unwrap();
        }

        install(&dir).unwrap();
        assert!(installed(&dir));
        uninstall(&dir).unwrap();
        assert!(!installed(&dir));
        assert!(!dir.join(PACKAGE).exists());
        for name in theirs {
            assert_eq!(fs::read_to_string(nca.join(name)).unwrap(), format!("# {name}\r\n"));
        }
        assert_eq!(fs::read_dir(&nca).unwrap().count(), theirs.len());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn uninstall_without_install_does_nothing() {
        let dir = game_dir("none");
        uninstall(&dir).unwrap();
        assert!(!dir.join("python_nca_internal").exists());
        fs::remove_dir_all(dir).unwrap();
    }
}
