# Repository Guidelines

## Project Structure & Module Organization

MediaDrop is a Tauri 2 desktop app with a vanilla web UI and Rust backend. Frontend files live in `src/`: `index.html`, `main.js`, `styles.css`, and image assets in `src/assets/`. Native code, app configuration, permissions, icons, and bundled command-line tools live in `src-tauri/`; the main Rust logic is in `src-tauri/src/lib.rs` and startup wiring is in `src-tauri/src/main.rs`. Windows-focused diagnostic helpers are in `tools/`. Release metadata and scripts are at the repository root, including `latest.json`, `release-notes.md`, `generate-latest.ps1`, and `release-mediadrop.ps1`.

## Build, Test, and Development Commands

- `npm install`: install the Tauri CLI and JavaScript dependencies.
- `npm run tauri dev`: launch the local Tauri app using `src/` as `frontendDist`.
- `npm run tauri build`: build the bundled Windows MSI and updater artifacts.
- `cd src-tauri; cargo test`: run Rust tests when present.
- `cd src-tauri; cargo fmt`: format Rust code before submitting changes.
- `.\generate-latest.ps1` / `.\release-mediadrop.ps1`: update release metadata and package releases; use from PowerShell on Windows.

## Coding Style & Naming Conventions

Use vanilla ES modules in `src/main.js`; prefer `const`/`let`, camelCase variables, and descriptive DOM IDs that match `index.html`. Keep UI behavior in JavaScript and presentation in `styles.css`. Rust code should follow `rustfmt`, use `snake_case` for functions and variables, and `PascalCase` for structs/enums. Keep Tauri commands and long-running process logic in `src-tauri/src/lib.rs`, and avoid spreading backend behavior into frontend-only files.

## Testing Guidelines

There is no dedicated frontend test runner configured. For backend changes, add focused Rust tests near the code they cover or in integration tests under `src-tauri/tests/` if a public API boundary is needed. Run `cargo test` before release-oriented changes. For UI changes, verify manually with `npm run tauri dev`, including downloads, cancellation, folder selection, updates, and clipped media flows.

## Commit & Pull Request Guidelines

Git history was not available in this environment, so use clear, imperative commit subjects such as `Fix paused download cleanup` or `Update release metadata`. Keep commits scoped to one concern. Pull requests should include a short summary, test or manual verification notes, linked issues when relevant, and screenshots or screen recordings for visible UI changes.

## Security & Configuration Tips

Do not add new secrets to source files. Review `src-tauri/tauri.conf.json` when changing CSP, updater endpoints, bundled binaries, or permissions. Avoid committing generated outputs such as `node_modules/` and `src-tauri/target/`.

## Agent Guidance

Keep this file limited to durable repository rules. Do not use `AGENTS.md` for task-by-task change logs, work summaries, or transient notes; report those in the conversation, commits, pull requests, or release notes instead.
