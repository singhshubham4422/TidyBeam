# Security policy

## Reporting a vulnerability

Please **don't open a public issue** for security problems. Use GitHub's private vulnerability reporting instead: open the repository's **Security** tab and choose **Report a vulnerability**.

Include what you found, how to reproduce it, the affected version and OS, and the impact you expect. We aim to acknowledge reports within 5 working days and to ship a fix for confirmed issues as quickly as their severity requires. We're happy to credit you in the release notes.

## Supported versions

Security fixes go into the latest release only.

## Security model

TidyBeam is a file manager, so it deliberately has broad access to the files of the user who runs it. The design depends on a few rules:

- **The web view is trusted code and must never load remote content.** The interface talks to Rust commands that can read, move and delete any file the user can. The Content Security Policy in `src-tauri/tauri.conf.json` only allows the bundled app, Tauri IPC and the local asset protocol. Don't add remote scripts, styles, fonts, iframes or URLs, and never render file contents as HTML.
- **No network access.** The app makes no network requests; there's no telemetry or update check. Search indexes contain file names and metadata only, never contents, and are stored in the user's local app-data folder.
- **Programs need confirmation.** Opening an executable or script (`.exe`, `.bat`, `.ps1`, `.sh`, `.command`, `.desktop`, `.app` and similar, or an extension-less Unix file with an execute bit) always asks the user first.
- **Deletion is recoverable by default.** Deletes go to the system Recycle Bin or Trash. Permanent deletion only happens after a separate confirmation when an item can't be trashed.
- **Archives are untrusted.** ZIP extraction rejects paths that escape the destination or cross symbolic links, refuses to overwrite existing files, checks free space, refuses entries with extreme compression ratios, and never writes more than an entry declares.
- **Transfers don't follow symbolic links**, refuse to copy a folder into itself, and refuse to replace a folder that contains the item being moved.

## Release integrity

Release builds are produced by GitHub Actions from tagged commits. Windows builds are Authenticode-signed and macOS builds are signed with a Developer ID and notarized when the signing secrets are configured. Every release publishes SHA-256 checksums.
