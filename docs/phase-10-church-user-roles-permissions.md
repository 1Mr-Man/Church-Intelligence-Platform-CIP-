# Phase 10: Church/User Roles & Permissions

## Baseline

Trigger: the user's explicit instruction, "Keep going into church/user
roles & permissions next" - item 5 in `docs/phase-4-master-plan-gap-audit.md`'s
own "Proposed Phase 4 candidates" list, graded `NOT STARTED`: *"No
multi-user model exists at all - the app has no login, no user table, no
role enforcement."* Full reasoning in `docs/phase-10-audit.md`, including
a clarification that the user's separately-pasted item list (numbered
9-12, referencing "real audio fingerprinting" as still-open) comes from
the external advice document's own numbering, not this project's actual
phase history - real audio fingerprinting shipped in Phase 7.1-7.3, and
Phase 8/Phase 9 (Production Integration; Bible Translation Registry v2)
were both already complete, committed, pushed, and Windows-verified
before this phase began.

## Design choices

See `docs/phase-10-audit.md` in full. Summary: two closed roles (Admin/
Operator, not a general RBAC framework); local PIN-based authentication
(`base64(sha256(salt || pin))`, the same algorithm shape Phase 8's OBS
auth already uses, chosen deliberately over `bcrypt`/`argon2` since the
threat model is a single local desktop machine, not a networked
credential); session-only login (`AppState.current_operator`, never
persisted, identical precedent to `screen_route_modes`); a bootstrap rule
(the first account ever created needs no login and always becomes Admin);
and a concrete, real gate on exactly the commands that are configuration
rather than day-to-day operation.

## What was built

- **`core/access`** (new crate): `Role`, `OperatorAccount`,
  `OperatorAccountStore` trait, `AccessError`, `generate_salt`/
  `hash_pin`/`verify_pin`.
- **`database/migrations/0017_operator_accounts.sql`**: the
  `operator_accounts` table (id, display_name unique, role, pin_hash,
  pin_salt, created_at).
- **`integrations/access`** (new crate): `SqliteOperatorAccountStore`,
  mirroring `SqliteContentRegistry`'s shape.
- **`apps/desktop/src-tauri/src/access.rs`** (new orchestration module):
  `OperatorSession`, `ensure_admin` (pure, fail-closed gate),
  `create_operator_account` (bootstrap logic), `login`.
- **`apps/desktop/src-tauri/src/commands.rs`**: `OperatorAccountSummaryDto`
  (never carries `pinHash`/`pinSalt`); 5 new commands
  (`list_operator_accounts`, `create_operator_account`, `login`,
  `logout`, `get_current_operator`); `ensure_admin`/`ensure_admin_string_err`
  gates added to the top of 7 existing commands (`import_bible_dataset`,
  `set_content_enabled`, `set_production_integration_config`,
  `generate_verse_embeddings`, `install_whisper_model`,
  `install_embedding_model_file`, `install_embedding_tokenizer_file`).
- **`apps/desktop/src-tauri/src/state.rs`**: `operator_account_store`
  (`Box<dyn OperatorAccountStore>`, its own connection) and
  `current_operator` (`Mutex<Option<OperatorSession>>`, session-only).
- **`apps/desktop/src-tauri/src/errors.rs`**: new `AppError::Forbidden`
  variant, categorized under `LogCategory::Security`.
- **Frontend**: `domain/access.ts` (`Role`, `OperatorAccountSummary`);
  5 new `lib/commands.ts` wrappers; new `components/LoginScreen.tsx` -
  a full-app gate mirroring `WebRuntimeNotice`'s existing pattern in
  `App.tsx`, rendering either a "create the first Admin account" form
  (zero accounts) or an ordinary login form (pick an account, enter its
  PIN); `App.tsx` gains the `currentOperator` state, the login gate, and
  a Log Out control in the nav bar.
- **`docs/roles-permissions.md`** (new): the permanent reference doc
  naming the mechanism, the exact gated-command table, and the explicit
  statement of what the login screen is (and isn't) a security boundary
  against.

## Testing boundary

`ensure_admin`/`create_operator_account`/`login` are pure functions
tested directly against a real in-memory SQLite store
(`cip_database::open_in_memory` + `run_migrations` +
`SqliteOperatorAccountStore`) - the same `resolve_default_translation_id_from_registry`/
`ensure_ai_processing_permitted` pattern this project already established
(no `tauri::test` harness exists). New Rust tests: 6 in `core/access`
(salt/hash/verify semantics, Role JSON round-trip), 6 in
`integrations/access` (SQLite CRUD, duplicate-name rejection, role
round-trip), 12 in `apps/desktop/src-tauri/src/access.rs` (`ensure_admin`
for no-session/Operator/Admin; the bootstrap rule end to end - first
account becomes Admin unconditionally, a second account requires a
logged-in Admin and is refused for no-session or an Operator session;
PIN-length and empty-name validation; login success, wrong-PIN failure,
and unknown-account-id failure sharing the same error shape as a wrong
PIN). Frontend: 6 new `commands.ts` wrapper tests (forwarding + outside-
Tauri rejection) and 2 new domain contract tests (`OperatorAccountSummary`
never carries a pin field; `Role` is exactly the two-value closed set).

## Full regression result

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets -- -D warnings`: clean, both
  feature configs (default and `--features whisper`).
- `cargo check --workspace` / `cargo check --features whisper`: clean.
- `cargo test --workspace`: 1001 passed, 0 failed (default config, up
  from Phase 9's 977 - 24 new).
- `cargo test --features whisper` (desktop crate): 332 passed, 0 failed
  (up from Phase 9's 320 - 12 new).
- `npm run typecheck` / `npm run lint` (5 pre-existing warnings,
  unchanged) / `npm run test -- --run` (274 passed, up from Phase 9's
  266 - 8 new) / `npm run build`: all clean.

## Architectural safety

- 5 new Tauri commands, zero new events, one new migration
  (purely additive - a new table, no existing table's shape changes).
- Every gated command's existing logic is unchanged below the new
  `ensure_admin`/`ensure_admin_string_err` call at its top - a build
  with a logged-in Admin behaves identically to before this phase for
  every one of the seven commands.
- No existing Rust test called any of the seven gated commands as a
  `#[tauri::command]` wrapper (verified before adding the gates) - they
  only exercise the pure functions beneath (`content::import_and_register`,
  `production::test_obs_connection`, `embeddings::generate_verse_embeddings_for_translation`,
  etc.), which remain untouched, so the new gates could not have broken
  any pre-existing test.
- `core/bible`/`core/service`/`core/presentation` (every domain contract
  crate) are entirely untouched.

## Windows rebuild

Required: this phase changes Rust code compiled into the desktop binary
(new migration, new crates, new gate calls in seven existing commands,
five new commands). See `pilot-evidence/10/windows/installer-contents-verification.json`
and the updated `release/windows/release-manifest.json`.

## Known limitations (honest, not deferred silently)

- **No password recovery.** An Admin locked out has no in-app recovery
  path - a future phase could add one (e.g. a special first-run reset),
  not attempted here.
- **No account editing/deactivation/deletion** - `list`/`create` only.
  An operator who has left cannot be removed from the account list
  today.
- **No per-action audit attribution.** `audit_events`/timeline already
  record *what* happened; they do not yet record *which* operator
  triggered it - would need `current_operator` threaded into every
  existing write path, a real, separate, future addition deliberately
  not attempted here to keep this phase's diff focused on the access-
  control mechanism itself.
- **The login screen is a workflow control, not itself the security
  boundary** - see `docs/roles-permissions.md`'s own explicit section on
  this; the real boundary is the seven `ensure_admin` calls inside the
  Rust commands.
- **No multi-church/multi-tenant data isolation** - still one SQLite
  database per install; "multi-user" here means multiple human operators
  of one installation, not multiple churches sharing one instance.
- **The `LoginScreen` UI was not visually verified via a real Xvfb+
  screenshot pass in this phase** - full Rust/TS regression (including
  every access-control code path: bootstrap, both gate outcomes, PIN
  verification success/failure) is green, but a real rendered screenshot
  of the login/account-creation forms was not captured, unlike the
  dedicated Xvfb+xdotool+screenshot passes this project has used for
  genuine window-placement/rendering-pipeline changes (e.g. Phase
  3.8.3). Judged proportionate: this is ordinary React form content, not
  a new windowing/pixel-placement mechanism - but stated honestly rather
  than silently implied verified.

## Final gate

Environment A (build-time verification, full regression, direct binary
symbol inspection): PASS. Environment C (a real operator creating the
first Admin account, logging in/out, and confirming a non-Admin account
is genuinely refused when attempting a gated action on real Windows
hardware): not yet performed - carried forward into
`physicalHardwareStatement` per this project's standing discipline.
