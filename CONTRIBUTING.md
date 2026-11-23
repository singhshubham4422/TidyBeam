# Contributing

Thanks for helping improve TidyBeam.

## Setup

Follow **Build from source** in the [README](README.md), then:

```sh
npm ci
npm run tauri dev
```

## Before you open a pull request

```sh
npm run check        # tsc, vite build, cargo fmt --check, clippy -D warnings, cargo test
```

- Keep changes focused; one topic per pull request.
- Add or update Rust tests for file-operation changes. Anything that moves, overwrites or deletes data needs a test that proves existing data is kept safe.
- Test on every OS you can. File systems differ: case sensitivity, path separators, hidden-file rules, trash behavior and file-watch limits all vary between Windows, macOS and Linux.
- Don't add network access, telemetry or remote web content (see [SECURITY.md](SECURITY.md)).
- User-facing text should be plain and short, and should use the platform's words (“Recycle Bin” on Windows, “Trash” elsewhere).

## Project layout

See [ARCHITECTURE.md](ARCHITECTURE.md).

## License

By contributing you agree that your contributions are licensed under the [MIT License](LICENSE).
