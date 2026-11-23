# Changelog

## 0.3.0 — unreleased

First cross-platform release candidate.

### Added
- Windows, macOS and Linux builds (MSI/NSIS, DMG, AppImage/deb/rpm) from CI.
- New interface: simpler layout, light and dark themes, accent colors, compact rows, Settings page.
- Real back/forward history per tab, breadcrumbs built from the actual path on every OS, `~` in the location box.
- Grid view with lazy image thumbnails; previews for images, text, video and audio.
- Shift-click range selection and keyboard navigation in file lists.
- Search: persistent indexed folders that survive restarts, catch up on changes made while the app was closed, and can be paused or removed. New `type:` and `modified:week` filters.
- Confirmation before moving to the Trash (configurable) and before running programs or scripts.
- Permanent delete offered, with confirmation, only when an item can't go to the Trash.

### Changed
- Sorting and filtering happen in Rust across the whole folder, not just the loaded rows.
- Folder listing reuses directory-read metadata; large files copy with 1 MiB chunks, small files use the OS copy routine.
- Duplicate finder hashes in parallel with a sampled pre-check before full SHA-256.
- Search index uses an external-content FTS5 table, separate read and write connections, batched commits and indexed path ranges.
- The global shortcut is now opt-in and configurable (previously Alt+Space, which clashed with the Windows window menu).

### Fixed
- Up, breadcrumbs and typed paths broke on macOS and Linux.
- Opening files failed on every OS.
- Rename could silently overwrite an existing file.
- A failed move could fall back to copy-and-delete for errors other than crossing drives.
- “Replace” could send the folder containing the source to the Trash.
- Copies lost permissions and timestamps; symbolic links aborted whole transfers.
- ZIP extraction had no size limits; ZIPs over 4 GB couldn't be created.
- Cleanup could list other users' files in shared `/tmp` and listed `C:\Windows\Temp`, which needs admin rights.
- Hidden files ignored the Windows hidden/system attributes and the macOS hidden flag.
- Linux drive list included snap, loop and system mounts.
- Ctrl+V blocked pasting text into fields; the location box didn't update after navigating.
