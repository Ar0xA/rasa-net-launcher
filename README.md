# Infinite Rasa Launcher

A Windows launcher for playing Tabula Rasa on [Infinite Rasa](https://infiniterasa.org).

It:

- finds your Tabula Rasa 1.16.5.0 install (or lets you pick the folder),
- checks the game files against a known-good manifest and lists anything missing or modified,
- shows community news and patch notes from the Infinite Rasa site,
- lets you pick an auth server (Infinite Rasa by default, or add your own, e.g. `localhost:2106`),
- can remember your username, and optionally your password, per server,
- updates itself automatically when a new release is published.

The launcher does **not** download, patch or repair game files. If files are reported missing or modified, reinstall a clean Tabula Rasa 1.16.5.0 client.

Saved passwords are stored in Windows Credential Manager, never in a file. When a password is saved, it is passed to the game on its command line at launch, which other programs on the same PC can read while the game runs.

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

## File manifest

`src-tauri/manifest.json` holds the path, size and SHA-256 of every file in a clean 1.16.5.0 client and is compiled into the launcher. Per-user files (logs, screenshots, `client.cfg`, `login.cfg`, …) are excluded. To regenerate it from a clean install:

```powershell
.\tools\New-Manifest.ps1 -GameDir "C:\path\to\Tabula Rasa 1.16.5.0"
```

## License

The launcher source code is released under the [MIT License](LICENSE).

The Infinite Rasa name and logo are not covered by that license. Tabula Rasa is a trademark of its respective owner; this project is not affiliated with or endorsed by NCsoft. No game files are included.
