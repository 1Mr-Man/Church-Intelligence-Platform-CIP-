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
| `cargo test -p cip-integration-tests`                  | Run only the cross-domain foundation wiring test          |
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
  low/medium/high bucketing, `core/bible/src/context.rs` proves the
  Scripture Context Manager trait is object-safe and wireable.
- **Database/migration tests** (`database/src/migrations.rs`,
  `database/src/seed.rs`) prove migrations are idempotent and that all ten
  Phase 1 tables exist after running them.
- **Cross-crate integration tests** (`tests/tests/foundation_wiring.rs`)
  prove the domains actually compose: a `ServiceSession` is started, a
  verse is resolved through the real SQLite-backed `BibleProvider`,
  wrapped in a `Suggestion`, approved into a `PresentationItem`, and handed
  to a `Renderer` - crossing every domain boundary using only public
  contracts.
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
