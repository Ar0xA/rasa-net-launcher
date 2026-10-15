# Infinite Rasa Launcher

A small Windows launcher for playing Tabula Rasa on [Infinite Rasa](https://infiniterasa.org).

It:

- finds your Tabula Rasa 1.16.5.0 install (or lets you pick the folder),
- checks the game files against a known-good manifest and lists anything missing or modified,
- optionally skips the intro movie,
- launches the game against the Infinite Rasa auth server.

The launcher does **not** download, patch or repair game files, and it never handles your account password — you log in in-game as usual. If files are reported missing or modified, reinstall a clean Tabula Rasa 1.16.5.0 client.

## Building

Prerequisites: [Rust](https://rustup.rs) (MSVC toolchain), Visual Studio Build Tools with "Desktop development with C++", and Node.js LTS.

```powershell
npm install
npm run tauri dev     # run with live reload
npm run tauri build   # produce the installer in src-tauri/target/release/bundle/nsis
```

## Configuration

- **Auth server** — `AUTH_SERVER` at the top of [`src-tauri/src/lib.rs`](src-tauri/src/lib.rs).
- **Version** — `version` in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and `package.json`.

## File manifest

`src-tauri/manifest.json` holds the path, size and SHA-256 of every file in a clean 1.16.5.0 client and is compiled into the launcher. Per-user files (logs, screenshots, `client.cfg`, `login.cfg`, …) are excluded. To regenerate it from a clean install:

```powershell
.\tools\New-Manifest.ps1 -GameDir "C:\path\to\Tabula Rasa 1.16.5.0"
```

## License

The launcher source code is released under the [MIT License](LICENSE).

The Infinite Rasa name and logo are not covered by that license. Tabula Rasa is a trademark of its respective owner; this project is not affiliated with or endorsed by NCsoft. No game files are included.
