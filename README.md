# TidyBeam

A fast, simple, local-first file manager for **Windows, macOS and Linux**, built with [Tauri 2](https://tauri.app), Rust and React.

- Tabs, dual pane, list and grid views with image thumbnails, a details panel with previews
- Back/forward history, breadcrumbs, favorites, drives, light and dark themes
- Copy and move with conflict handling, pause/resume/cancel and live progress
- Instant name search across folders you choose to index (SQLite FTS5, stored on your device)
- Space analyzer, exact duplicate finder, temporary-file cleanup, ZIP browse/create/extract
- Rules that move files by type and age, manually after a preview or automatically
- Deletes go to the system Recycle Bin / Trash; nothing is removed permanently without asking

Nothing leaves your computer: there is no telemetry, account or network access.

## Install

Download the installer for your platform from the [Releases page](https://github.com/singhshubham4422/TidyBeam/releases):

| Platform | Files |
|---|---|
| Windows 10/11 x64 | `.msi` or `-setup.exe` |
| Windows 11 ARM64 | `arm64-setup.exe` |
| macOS 10.15+ Apple silicon | `aarch64.dmg` |
| macOS 10.15+ Intel | `x64.dmg` |
| Linux x64 | `amd64.AppImage`, `amd64.deb` or `x86_64.rpm` |
| Linux ARM64 | `aarch64.AppImage`, `arm64.deb` or `aarch64.rpm` |

Each release lists SHA-256 checksums so you can verify downloads.

## Build from source

You need [Node.js](https://nodejs.org) 20+ and [Rust](https://rustup.rs) 1.85+, plus the Tauri system dependencies for your OS:

- **Windows:** Microsoft C++ Build Tools and WebView2 (preinstalled on Windows 11).
- **macOS:** Xcode Command Line Tools (`xcode-select --install`).
- **Linux (Debian/Ubuntu):**
  ```sh
  sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```
  For Fedora, Arch and others see the [Tauri prerequisites](https://tauri.app/start/prerequisites/).

Then:

```sh
npm ci
npm run tauri dev      # run in development
npm run tauri build    # build installers for the current OS into src-tauri/target/release/bundle
```

Run every check (type-check, frontend build, Rust format, Clippy, tests):

```sh
npm run check
```

## Keyboard shortcuts

| Action | Windows / Linux | macOS |
|---|---|---|
| Search everywhere | Ctrl+K | ⌘K |
| Type a location | Ctrl+L | ⌘L |
| Back / forward | Alt+← / Alt+→ | ⌘[ / ⌘] |
| Up one folder | Alt+↑ | ⌘↑ |
| New tab / close tab | Ctrl+T / Ctrl+W | ⌘T / ⌘W |
| New folder | Ctrl+Shift+N | ⌘⇧N |
| Rename | F2 | F2 |
| Move to Trash | Delete | ⌘⌫ |
| Copy / cut / paste | Ctrl+C / X / V | ⌘C / X / V |
| Undo rename or new folder | Ctrl+Z | ⌘Z |
| Dual pane | Ctrl+\\ | ⌘\\ |

A system-wide search shortcut can be turned on in **Settings → Shortcuts**. It's off by default because it can clash with other apps, and some Wayland desktops don't allow it.

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md) — how the app is put together
- [CONTRIBUTING.md](CONTRIBUTING.md) — development workflow
- [SECURITY.md](SECURITY.md) — reporting vulnerabilities and the security model
- [CHANGELOG.md](CHANGELOG.md)

## License

[MIT](LICENSE)
