# TidyBeam — Branding, Store Listing & Cross-Platform Publishing Plan

This guide consolidates TidyBeam's **brand positioning, vendor-free taglines, Store copy kit, platform certification rules, and multi-OS release workflow** for **Windows (Microsoft Store + NSIS/MSI), macOS (DMG), and Linux (AppImage / `.deb` / `.rpm`)**.

---

## 1. Brand Identity & Tagline Kit

All copy keeps TidyBeam's core tone from [README.md](README.md) and [PRIVACY.md](PRIVACY.md): plain-spoken, vendor-free, local-first, and cross-platform.

### Top Picks (Primary Placements)

- **Best Overall (Short & Balanced):**  
  > **Fast file management, deep cleanup, and system health — built local-first.**

- **GitHub [README.md](README.md) & Landing Hero:**  
  > **Clean storage, faster systems, effortless files — across Windows, macOS, and Linux.**  
  > *Everything runs on your device — no account, no telemetry, no cloud.*

- **Microsoft Store & [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) (`shortDescription`):**  
  > **Fast, local-first file manager, deep cleaner, and system optimizer.**

- **Recommended Headline + Supporting Line Pairing (Website, GitHub, Store):**
  - **Main Headline:** *Clean storage, effortless files, and safer system care.*
  - **Supporting Line:** *A fast, local-first toolkit for Windows, macOS, and Linux — no account, no telemetry, no network.*

### Secondary Options by Context

#### Punchy 4–6 Word Slogans (Logos, Banners, Social Cards)
- **Tidy files. Fast storage. Healthier system.**
- **Every file, cleaned and fast.**
- **Deep clean, instant files, zero clutter.**
- **Your system, tidy at full speed.**

#### Privacy & Offline-First Angle
- **Clean your system and organize your files — completely offline.**
- **Private storage cleanup, instant file search, and system care on your device.**
- **Zero-telemetry file manager, deep cleaner, and performance toolkit.**

#### All-in-One Cross-OS Utility Angle
- **The fast, private toolkit to organize files, reclaim space, and tune your system.**
- **One fast app to browse files, clear clutter, and keep your machine running smooth.**
- **Cross-platform file manager, storage analyzer, and safe system cleaner.**

---

## 2. Store & Manifest Metadata Alignment

When preparing a release with expanded storage, cleanup, and system health features, update copy consistently across these exact files:

| File | Field / Location | Recommended Copy |
|---|---|---|
| [`README.md`](README.md) | Hero subtitle | `Clean storage, faster systems, effortless files — across Windows, macOS, and Linux. Everything runs on your device — no account, no telemetry, no cloud.` |
| [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) | `bundle.shortDescription` | `Fast, local-first file manager, deep cleaner, and system optimizer.` |
| [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) | `bundle.longDescription` | `TidyBeam is a fast, local-first toolkit for Windows, macOS, and Linux with tabbed file browsing, instant search, high-speed storage analysis, duplicate detection, deep system and browser cleanup, startup and process tools, and safe automation rules.` |
| [`packaging/msix/AppxManifest.xml`](packaging/msix/AppxManifest.xml) | `<Description>` | `Clean storage, effortless files, and safer system care — local-first with zero telemetry.` |
| [`package.json`](package.json) & [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml) | `description` | `Fast, local-first file manager, deep cleaner, and system optimizer for Windows, macOS, and Linux.` |

### Microsoft Store Listing Copy Template (`9NXSC9HV5S12`)

- **Store Title:** `TidyBeam` (Package Family: `SwiftLabs.TidyBeam_eha7verb3j2br`, see [docs/microsoft-store.md](docs/microsoft-store.md))
- **Short Summary:** `Fast, local-first file manager, deep cleaner, and system optimizer.`
- **Store Description Pillars:**
  1. **Browse & Organize Effortlessly:** Tabs, dual panes, instant local file search, ZIP archive tools, and automated organize rules.
  2. **Reclaim Storage Fast:** High-speed disk space analyzer, exact duplicate finder, developer cache cleanup, and large-file discovery.
  3. **Safe System & Privacy Cleanup:** Clean temporary files, browser caches, app leftovers, and SQLite databases—recoverable by default via Recycle Bin before permanent deletion.
  4. **System Health & Performance:** One-click health summary, startup manager, live CPU/RAM/process monitor, battery health, and hardware overview.
  5. **100% Local & Private:** Zero accounts, zero telemetry, zero ads, and no background network activity.

---

## 3. Platform Certification & Distribution Planning

### 3.1 Windows (Microsoft Store MSIX + Direct NSIS/MSI)

- **Build Pipeline:** [`.github/workflows/store.yml`](.github/workflows/store.yml) compiles `x86_64-pc-windows-msvc` and `aarch64-pc-windows-msvc`, packs each into `.msix` using `makeappx.exe`, merges into `TidyBeam_<version>.msixbundle`, and submits via `msstore publish`. Direct `.exe` / `.msi` installers build via [`.github/workflows/release.yml`](.github/workflows/release.yml).
- **Store Policy Safeguards (Avoid Rejections):**
  - **No UAC / Kernel Drivers on Launch:** Keep `runFullTrust` standard-user only; do not bundle ring-0 temperature drivers or silently trigger elevation prompts.
  - **No Deceptive AV or Pop-up Claims:** Present security checks strictly as read-only Windows Security / Update posture shortcuts—never as an independent antivirus engine or third-party ad injector.
  - **Accurate `runFullTrust` Justification:** *"TidyBeam is a local-first desktop file manager, storage analyzer, and user-scoped system cleaner that needs standard desktop access to manage the user's files, folders, and local caches."*
  - **Separate Store vs Non-Store Capabilities:** Gate features requiring admin elevation (e.g., `C:\Windows\Temp`, optional raw SMART queries, or portable mode) cleanly off or behind a standard-user boundary inside MSIX packages.
  - **Monotonic Store Versions:** Every Store submission uses `major.minor.patch.0` and must strictly exceed the previous published version.
- **Direct Installer Signing:** Configure `WINDOWS_CERTIFICATE` (base64 `.pfx`) and `WINDOWS_CERTIFICATE_PASSWORD` secrets in GitHub so direct `.exe`/`.msi` builds carry valid Authenticode signatures and avoid SmartScreen warnings.

### 3.2 macOS (Apple Silicon + Intel `.dmg`)

- **Build Pipeline:** [`.github/workflows/release.yml`](.github/workflows/release.yml) builds `aarch64-apple-darwin` and `x86_64-apple-darwin` targets (`minimumSystemVersion: "10.15"`).
- **Signing & Notarization:** Configure `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, and `APPLE_TEAM_ID` secrets so `.dmg` releases pass macOS Gatekeeper without user bypasses.
- **TCC / Permissions UX:** Keep standard browsing, storage analysis, and app cache cleanup working out of the box; show a clear, optional guide only when a user requests protected Safari/Trash cleanup that requires **Full Disk Access** in System Settings.

### 3.3 Linux (`AppImage`, `.deb`, `.rpm`)

- **Build Pipeline:** [`.github/workflows/release.yml`](.github/workflows/release.yml) produces x64 and ARM64 `AppImage`, `.deb`, and `.rpm` artifacts.
- **Desktop Compatibility:**
  - Verify `libwebkit2gtk-4.1` and `libayatana-appindicator3` dependencies across Ubuntu/Debian/Fedora.
  - Use `pkexec` strictly on demand for optional root actions (`apt`/`dnf`/`pacman` cache cleanup, `journalctl` vacuum).
  - Gracefully disable global floating-bar positioning and note global-shortcut limitations on Wayland desktops (consistent with [SettingsView.tsx](src/views/SettingsView.tsx)).
- **Future Expansion (Post-v0.4+):** Consider publishing verified packages on **Flathub** (noting Flatpak filesystem portal limits vs full-disk cleanup) and **AUR** (`tidybeam-bin`).

---

## 4. Pre-Release & Publish Checklist

Run this sequence for every release cycle:

### Step 1: Version & Documentation Sync
- [ ] Bump version identically in [`package.json`](package.json), [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml), and [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) (ensure `Cargo.lock` updates if Cargo dependencies/version changed).
- [ ] Update [`CHANGELOG.md`](CHANGELOG.md) with user-facing highlights under the new version heading.
- [ ] Audit [`PRIVACY.md`](PRIVACY.md) and [`SECURITY.md`](SECURITY.md) against any new features (update the `Last updated` date in `PRIVACY.md` if disclosures changed).
- [ ] Verify stable download filenames match [`README.md`](README.md) (`TidyBeam-<asset>[setup][ext]`).

### Step 2: Quality & Security Verification
- [ ] Run `npm run check` (`tsc`, `vite build`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`).
- [ ] Confirm [`.github/workflows/ci.yml`](.github/workflows/ci.yml) passes on `windows-latest`, `macos-latest`, and `ubuntu-22.04`, including `npm audit --omit=dev` and `cargo deny`.
- [ ] Refresh Store & README screenshots showing Light + Dark themes, Space Analyzer, Deep Cleanup preview, Health Check, and System Monitor.

### Step 3: Tag, Smoke-Test & Publish
- [ ] For beta testing: push a prerelease tag (e.g., `v0.4.0-beta.1`) — triggers draft GitHub release + `SHA256SUMS.txt` without auto-submitting to the Microsoft Store.
- [ ] For stable release: push `v0.4.0` — wait for all 6 platform builds + checksums in the draft GitHub release.
- [ ] Smoke-test installers on Windows 11, macOS, and Ubuntu/Fedora (verify clean install, uninstall, Trash-first cleanup safety, and Storage scan speed).
- [ ] **Publish** the GitHub release — this automatically triggers [`.github/workflows/store.yml`](.github/workflows/store.yml) to build and submit the `msixbundle` to Microsoft Partner Center.
- [ ] Monitor Partner Center certification (typically 1–3 business days) until status shows **In the Microsoft Store**.

---

## 5. Post-Publish Rollout & Visual Assets Checklist

Prepare clean, consistent marketing assets for GitHub Releases and Microsoft Store:

1. **Store Icons (already mapped in [`store.yml`](.github/workflows/store.yml)):** `StoreLogo.png`, `Square44x44Logo.png`, `Square71x71Logo.png`, `Square150x150Logo.png`, `Square310x310Logo.png`.
2. **Recommended Store Screenshots (16:9, 1920×1080 or 2560×1440, dark & light mix):**
   - **Shot 1 (Hero):** Dual-pane File Browser + Details Preview (*"Fast, private file browsing"*).
   - **Shot 2 (Storage):** Space Analyzer with instant drill-down & Treemap (*"See what takes space in seconds"*).
   - **Shot 3 (Cleanup):** Grouped Deep Cleanup + Safety Badges + Preview (*"Clean caches & duplicates safely"*).
   - **Shot 4 (Health & Boost):** One-Click Health Check + Live CPU/RAM/Startup Monitor (*"Tune startup & monitor system health"*).
   - **Shot 5 (Search & Rules):** `Ctrl/⌘ + K` Command Palette + Automate Rules (*"Instant offline search & smart rules"*).
3. **Announcement Blurb Template:**
   > **TidyBeam vX.Y.Z is live — Clean storage, effortless files, and safer system care across Windows, macOS, and Linux.**  
   > Everything runs locally on your device with zero accounts, zero telemetry, and Trash-safe defaults.  
   > Download from GitHub Releases or the Microsoft Store.
