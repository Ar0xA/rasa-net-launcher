# Infinite Rasa Launcher

A Windows launcher for playing Tabula Rasa on [Infinite Rasa](https://infiniterasa.org).

It:

- finds your Tabula Rasa 1.16.5.0 install (or lets you pick the folder),
- checks the game files against a known-good manifest and lists anything missing or modified,
- shows community news and patch notes from the Infinite Rasa site,
- lets you pick an auth server (Infinite Rasa by default, or add your own, e.g. `localhost:2106`),
- can remember your username, and optionally your password, per server,
- updates itself automatically when a new release is published.
- can install or remove the slow mouse look fix (see below).

The launcher does **not** download, patch or repair game files. If files are reported missing or modified, reinstall a clean Tabula Rasa 1.16.5.0 client.

Saved passwords are stored in Windows Credential Manager, never in a file. When a password is saved, it is passed to the game on its command line at launch, which other programs on the same PC can read while the game runs.

## Slow mouse look fix

The fix comes from **thatawesomeguy** on the Infinite Rasa Discord. In mouse look the client drops cursor moves of 1 UI unit, so slow mouse movement is lost, worst at high resolutions. The fix keeps such moves until they add up to more than 1 unit.

**Install mouse fix** on the main screen (below **Settings**) puts it in the game folder; the same button then reads **Remove mouse fix** and takes it out again. It works from the next game start.

The fix is two small Python files plus an import block (`src-tauri/mods/slowmouselook/`, `src-tauri/src/mouselook.rs`), no binaries. They go to `python_nca_internal/client_nca_internal/` in the game folder, which the client loads at start. If another mod already uses that folder, the launcher adds its import lines to that `__init__.py` as a marked block and removes only that block again (the launcher's `developerkeys.py` then stays, because the client needs it while that package exists); a `developerkeys.py` that is not the launcher's is left alone. A copy installed by hand from the fix's own zip shows as installed and can be removed with the same button. Known limit (from the fix itself): when one axis turns, a 1-unit move on the other axis is still dropped.

## Building

Prerequisites: [Rust](https://rustup.rs) (MSVC toolchain), Visual Studio Build Tools with "Desktop development with C++", and Node.js LTS.

```powershell
npm install
npm run tauri dev     # run with live reload
npm run tauri build   # produce the installer in src-tauri/target/release/bundle/nsis
```

## Releasing

Push a version tag and GitHub Actions builds the installer and publishes a GitHub Release with the setup `.exe`, `SHA256SUMS.txt` and the updater's `latest.json`:

```powershell
git tag v1.1.0
git push origin v1.1.0
```

The version comes from the tag, so there is no need to edit the version fields first. Installed launchers pick up the new release on their next start.

### Updater signing

Updates are signed so launchers only install releases you published. The public key is `plugins.updater.pubkey` in `src-tauri/tauri.conf.json`; the private key and its password live only in the repository secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Keep a backup of the private key: without it, existing installs can no longer be updated.

## Configuration

- **Default auth server** — `Settings::default` in [`src-tauri/src/settings.rs`](src-tauri/src/settings.rs).
- **News feed** — `NEWS_FEED` in [`src-tauri/src/news.rs`](src-tauri/src/news.rs).
- **Version** — set from the release tag in CI; for local builds, `version` in `src-tauri/tauri.conf.json`.

## Linux

The launcher also builds and runs on Linux. It starts the Windows client through Wine, or through an installed [Lutris](https://lutris.net) game (native or Flatpak); pick one under **Settings → Launch with**.

- **Wine** runs `tabula_rasa.exe` with the given Wine binary and `WINEPREFIX`.
- **Lutris** cannot pass arguments at launch, so before each launch the launcher sets `exe`, `args` and `working_dir` in the chosen game's `.yml` and then opens `lutris:rungameid/<id>`. The original file is kept once as `<file>.yml.irl-backup`. A saved password is never written there, so in this mode you enter it in-game.

Saved passwords are stored in the desktop keyring (Secret Service). Releases include an AppImage and a .deb; self-update works for the AppImage (a .deb is updated by installing the new one).

Wine and Lutris output from the last launch goes to `game.log` in `~/.local/share/org.infiniterasa.launcher/logs/`.

Prerequisites: Rust, Node.js LTS and Tauri's [Linux dependencies](https://tauri.app/start/prerequisites/#linux); on Debian/Ubuntu also `libdbus-1-dev`.

```sh
npm install
npm run tauri build -- --bundles deb,appimage
```

## File manifest

`src-tauri/manifest.json` holds the path, size and SHA-256 of every file in a clean 1.16.5.0 client and is compiled into the launcher. Per-user files (logs, screenshots, `client.cfg`, `login.cfg`, …) are excluded. To regenerate it from a clean install:

```powershell
.\tools\New-Manifest.ps1 -GameDir "C:\path\to\Tabula Rasa 1.16.5.0"
```

## License

The launcher source code is released under the [MIT License](LICENSE).

The Infinite Rasa name and logo are not covered by that license. No game files are included.

## Legal notice

Richard Garriott’s Tabula Rasa and its derivatives are Copyright of NC Interactive, LLC. Tabula Rasa and all associated logos or designs are trademarks or registered trademarks of NC Corporation. All rights reserved.

Infinite Rasa is a non-commercial, community-run fan project. It is not affiliated with, endorsed by, or operated under license from NC Corporation, NC Interactive, LLC or Destination Games.
