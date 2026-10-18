use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const GAME_EXE: &str = "tabula_rasa.exe";

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

#[derive(Serialize)]
pub struct VerifyResult {
    pub version: String,
    pub checked: usize,
    pub missing: Vec<String>,
    pub corrupt: Vec<String>,
}

pub fn is_game_dir(dir: &Path) -> bool {
    resolve(dir, GAME_EXE).is_file()
}

/// `dir/rel`, matching each component case-insensitively where the exact name is
/// missing: Windows ignores case, so a copied install may differ on Linux.
pub fn resolve(dir: &Path, rel: &str) -> PathBuf {
    let exact = dir.join(rel);
    if cfg!(windows) || exact.exists() {
        return exact;
    }
    let mut path = dir.to_path_buf();
    for part in rel.split('/') {
        let found = fs::read_dir(&path).ok().and_then(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name())
                .find(|n| n.to_string_lossy().eq_ignore_ascii_case(part))
        });
        path.push(found.as_deref().unwrap_or(part.as_ref()));
    }
    path
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

/// No registry outside Windows: look where Wine and Lutris installs usually are.
#[cfg(not(windows))]
fn registry_candidates() -> Vec<PathBuf> {
    crate::runner::game_dir_candidates()
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

pub fn detect_game_dir() -> Option<PathBuf> {
    registry_candidates()
        .into_iter()
        .chain(common_candidates())
        .find(|d| is_game_dir(d))
}

fn manifest() -> Result<Manifest, String> {
    // PowerShell writes a UTF-8 BOM; serde_json rejects it.
    serde_json::from_str(MANIFEST_JSON.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("Bad embedded manifest: {e}"))
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

/// Compares the install against the manifest. `full` hashes every file;
/// otherwise only presence and size are checked.
pub fn verify(dir: &Path, full: bool, mut progress: impl FnMut(usize, usize)) -> Result<VerifyResult, String> {
    let manifest = manifest()?;
    let total = manifest.files.len();
    let mut result = VerifyResult {
        version: manifest.version,
        checked: total,
        missing: vec![],
        corrupt: vec![],
    };

    for (i, entry) in manifest.files.iter().enumerate() {
        progress(i, total);
        let path = resolve(dir, &entry.path);
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

    progress(total, total);
    Ok(result)
}

const CLIENT_CFG: &str = "client.cfg";
const INTRO_KEY: &str = "Client.UserInterface.ShowIntroMovie";

fn cfg_key_of(line: &str) -> Option<&str> {
    line.split_once('=').map(|(k, _)| k.trim())
}

/// True if client.cfg has the intro movie turned off. A missing key means the game default (intro on).
pub fn skip_intro(dir: &Path) -> bool {
    fs::read_to_string(resolve(dir, CLIENT_CFG))
        .ok()
        .and_then(|cfg| {
            cfg.lines()
                .find(|l| cfg_key_of(l) == Some(INTRO_KEY))
                .map(|l| l.split_once('=').unwrap().1.trim().eq_ignore_ascii_case("false"))
        })
        .unwrap_or(false)
}

/// Sets ShowIntroMovie in client.cfg, leaving every other line untouched.
pub fn write_skip_intro(dir: &Path, skip: bool) -> Result<(), String> {
    let path = resolve(dir, CLIENT_CFG);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_names_in_any_case() {
        let dir = std::env::temp_dir().join(format!("irl-case-{}", std::process::id()));
        fs::create_dir_all(dir.join("components")).unwrap();
        fs::write(dir.join("components/Localization.INI"), "").unwrap();
        fs::write(dir.join("Tabula_Rasa.exe"), "").unwrap();
        assert!(resolve(&dir, "Components/localization.ini").is_file());
        assert!(is_game_dir(&dir));
        assert_eq!(resolve(&dir, "Missing/file"), dir.join("Missing/file"));
        fs::remove_dir_all(dir).unwrap();
    }
}
