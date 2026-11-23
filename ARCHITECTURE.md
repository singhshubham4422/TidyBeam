# Architecture

```text
React + TypeScript (src/)
  App.tsx            shell: tabs, history, keyboard, dialogs, transfers, settings wiring
  components/        FileList (virtualized, paged), Details, Sidebar, Palette, Tray, dialogs
  views/             Search, Storage, Duplicates, Cleanup, Rules, Settings
  api.ts             typed wrappers for every Rust command
  settings.ts        preferences in localStorage; theme/accent/density on <html>
        │  Tauri IPC (invoke + events)
Rust (src-tauri/src/)
  lib.rs             command layer; heavy work runs on blocking worker threads
  platform.rs        paths (no \\?\ prefixes), hidden/executable rules, name validation, trash, mounts
  listing.rs         folder snapshots, backend sort/filter/paging, path info, folder compare
  transfer.rs        copy/move engine with conflicts, metadata, pause/cancel, cross-drive moves
  archive.rs         ZIP list/extract (path, size, ratio checks) and create (Zip64)
  analysis.rs        space analysis (jwalk), duplicates (rayon + xxh3 + SHA-256), cleanup review
  index.rs           SQLite FTS5 name index, persistent roots, notify watchers
  rules.rs           organize rules, previews, background watchers
```

## Listing and performance

- `list_entries` reads a folder once, keeps a bounded snapshot (8 folders, 400k entries total, 250k per folder, 20 s TTL, invalidated by the folder's modified time), and serves pages of up to 1000 rows. Sorting (natural, folders first) and filtering run over the whole snapshot in Rust; sort orders are cached per snapshot.
- On Windows `DirEntry::metadata` comes from the directory read itself, so listing doesn't stat each file.
- The UI virtualizes rows and tiles and only fetches pages the viewport needs. Thumbnails and media previews load straight from disk through Tauri's asset protocol with `loading="lazy"`, so no file bytes cross IPC as base64.
- Files up to 16 MiB copy with `std::fs::copy`, which uses each OS's fast path (copy-on-write clones where available). Larger files copy in 1 MiB chunks so transfers can report progress and be paused or cancelled. Permissions and timestamps are preserved.

## Data safety

- Rename and create never overwrite. Transfers check conflicts up front and offer skip, keep both, or replace; a replaced item goes to the trash first, and a folder that contains the source can't be replaced.
- Moves use `rename`; only a cross-device error falls back to copy-then-delete. The original is removed only after the copy succeeds, and kept if any symbolic link couldn't be carried over.
- Trashing is per item; anything that can't be trashed is reported and only deleted permanently after the user confirms.

## Search index

- Stored at `<app local data>/search-index.sqlite3` (WAL). Schema version 2 uses a `files` table with a unique path and an external-content FTS5 table kept in sync by triggers, so deletes are cheap.
- Indexed roots are saved in a `roots` table. On launch, watchers are restored and each root is re-walked in the background to catch changes made while the app was closed. A re-walk upserts with a generation number and sweeps rows it didn't see.
- Separate read and write connections let searches run while indexing commits in batches of 25k rows.
- Live updates use `notify` recursive watchers. Only created or renamed folders are re-walked; a rescan event triggers a full re-walk. Hitting the Linux inotify limit is reported with a clear message.

## Security

See [SECURITY.md](SECURITY.md). The web view loads only bundled code under a strict CSP; opening files goes through a Rust command that requires confirmation for programs and scripts.

## Testing and CI

- `npm run check` runs TypeScript, the Vite build, `cargo fmt --check`, Clippy with warnings denied, and the Rust tests (transfers, archives, rename conflicts, listing, index, rules, analysis).
- `.github/workflows/ci.yml` runs those checks on Windows, macOS and Ubuntu, plus `cargo audit`, `cargo deny` and `npm audit`.
- `.github/workflows/release.yml` builds and publishes installers for all three platforms from a version tag, with optional code signing.
