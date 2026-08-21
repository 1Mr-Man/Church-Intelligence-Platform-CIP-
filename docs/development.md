# Development setup

## Prerequisites

- [Node.js](https://nodejs.org/) 20+ and [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/tools/install) (stable, via `rustup`)
- Tauri's platform dependencies - see the
  [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/).
  On Debian/Ubuntu:
  ```sh
  sudo apt-get install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
    librsvg2-dev patchelf libssl-dev libgtk-3-dev
  ```

No API keys or secrets are required to run CIP - it has no required cloud
dependency.

## Install

```sh
pnpm install
```

This installs both the workspace's JS dependencies and (indirectly)
resolves the Rust workspace the first time you build it.

## Commands

Run from the repository root unless noted.

| Command                                              | What it does                                             |
| ------------------------------------------------------ | ------------------------------------------------------------ |
| `pnpm --filter @cip/desktop tauri dev`                | Run the desktop app in development mode (hot reload)     |
| `pnpm --filter @cip/desktop tauri build`              | Build a release bundle                                    |
| `pnpm --filter @cip/desktop typecheck`                | TypeScript type-check (`tsc -b`)                          |
| `pnpm --filter @cip/desktop test`                     | Frontend unit/type tests (Vitest)                         |
| `pnpm --filter @cip/desktop lint`                     | Lint the frontend (oxlint)                                 |
| `cargo check --workspace`                             | Type-check every Rust crate                                |
| `cargo test --workspace`                               | Run every Rust unit + integration test                     |
| `cargo test -p cip-integration-tests`                  | Run cross-domain tests, including the Bible Intelligence Core acceptance test |
| `cargo test -p cip-core-bible`                          | Run normalization/detection/context-manager unit tests    |
| `cargo test -p cip-core-service`                        | Run the Bible Intelligence Core orchestrator's unit tests  |
| `cargo test -p cip-database`                           | Run only the migration/seed tests                          |
| `cargo fmt --all`                                       | Format all Rust code                                        |

## Environment

`CIP_ENV` selects the runtime environment (`development` / `test` /
`production`); it defaults to `development` for debug builds and
`production` for release builds. In `development` and `test`, the app
seeds a handful of verses on first launch so there's something to look at
- see [`docs/database.md`](database.md) - this never happens in
`production`.

## Project structure

See the top-level layout in the [README](../README.md#repository-layout)
and the domain/boundary breakdown in
[`docs/architecture.md`](architecture.md).

## Testing

See [`docs/architecture.md`](architecture.md) for what each domain owns.
Test coverage by layer:

- **Rust unit tests** live next to the code they test (`#[cfg(test)] mod tests`
  in each crate) - e.g. `core/confidence/src/lib.rs` tests the
  low/medium/high bucketing, `core/bible/src/detection.rs` tests reference
  detection shapes, `core/bible/src/context_manager.rs` tests the real
  Scripture Context Manager (replacement, ambiguity, bounded history), and
  `core/service/src/bible_intelligence.rs` tests the full orchestrator
  against an in-memory `BibleProvider` - see
  [`docs/bible-intelligence.md`](bible-intelligence.md) for what each of
  these covers.
- **Database/migration tests** (`database/src/migrations.rs`,
  `database/src/seed.rs`) prove migrations are idempotent and that all ten
  Phase 1 tables exist after running them.
- **Cross-crate integration tests** (`tests/tests/foundation_wiring.rs`,
  `tests/tests/bible_intelligence_acceptance.rs`) prove the domains
  actually compose against the real SQLite-backed `BibleProvider`: a
  `ServiceSession` is started, verses are resolved and wrapped in
  `Suggestion`s, one is approved into a `PresentationItem` and handed to a
  `Renderer`, and a realistic multi-segment pastoral-speech sequence
  (Romans 8 -> several unrelated segments -> verse 28/31/18 -> John 3 ->
  verse 16) resolves deterministically end to end.
- **Frontend/type tests** (`apps/desktop/src/**/*.test.ts`, run via
  `pnpm --filter @cip/desktop test`) cover the TypeScript mirrors of the
  Rust domain contracts and the event name registry; `tsc -b` is the type
  test for everything else.

Run everything before pushing:

```sh
cargo test --workspace
pnpm --filter @cip/desktop typecheck
pnpm --filter @cip/desktop test
```
