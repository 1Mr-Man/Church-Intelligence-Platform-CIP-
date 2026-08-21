# CIP desktop app

The Tauri + React + TypeScript desktop shell for the Church Intelligence
Platform. See the [repository root README](../../README.md) for project
purpose and [`docs/`](../../docs) for architecture, setup, and command
reference - this package doesn't duplicate that here.

- `src/` - React/TypeScript frontend, including the domain contract
  mirrors (`src/domain/`) and event name registry (`src/events/`).
- `src-tauri/` - Rust backend: Tauri commands, app configuration, logging
  categories, and the event bus wiring. Depends on the workspace's
  `core/*`, `database`, and `integrations/*` crates.
