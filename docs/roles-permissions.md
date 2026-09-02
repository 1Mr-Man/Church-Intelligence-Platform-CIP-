# Church/User Roles & Permissions

CIP is a single local desktop installation - one church, one machine, no
cloud, no network dependency. "Multi-user" here (per
`docs/phase-4-master-plan-gap-audit.md`'s own framing) means separate
human operators of that one installation, across services and seasons:
a tech lead or pastor who configures licensing, OBS/vMix credentials,
and AI model files, and a rotating cast of Sunday volunteers who run the
live service without ever touching those settings. This document names
the mechanism Phase 10 built for that; see `docs/phase-10-audit.md` for
the full design record and `docs/phase-10-church-user-roles-permissions.md`
for the phase report.

## Roles

Exactly two, a closed set (`cip_core_access::Role`):

- **Admin** - everything an Operator can do, plus the commands in the
  table below.
- **Operator** - day-to-day live-service operation: search, display,
  approve/reject AI suggestions, start/pause/resume a service, and every
  command not explicitly Admin-gated.

A third role (e.g. a read-only "Viewer") is a reasonable future
addition, not designed here - nothing in this codebase needs it yet.

## Accounts

One `operator_accounts` row per local operator (migration 0017): a
display name (unique), a role, and a PIN, hashed as
`base64(sha256(salt || pin))` with a random per-account salt - see
`cip_core_access::hash_pin`'s own docs for why this algorithm (not
`bcrypt`/`argon2`) is the right, honestly-scoped choice for a PIN
protecting a single local desktop machine, not a networked credential.

**Bootstrap.** The very first account ever created (an empty
`operator_accounts` table) requires no login and always becomes Admin,
regardless of the role requested - there is no other way for the first
account to come into existence. Every account after that requires a
logged-in Admin to create (`access::create_operator_account`).

**Session.** `AppState.current_operator` is in-memory only, never
persisted - a restart always requires logging in again, the same
precedent as `screen_route_modes` (Presentation Router's Live/Held
state). The frontend's `LoginScreen` gates the entire app behind this:
no operator selected and authenticated, nothing else renders.

## What requires Admin

| Command | Why it's gated |
|---|---|
| `import_bible_dataset` | Bulk-imports Bible text - the exact action `LicensingStatus`'s admission gate protects. |
| `set_content_enabled` | Enables/disables installed content (translations, music datasets). |
| `set_production_integration_config` | Stores OBS/vMix host/port/credentials. |
| `generate_verse_embeddings` | Sends Bible text into a local AI embedding model - Phase 9's own `ai_processing_allowed` gate sits behind this same Admin check as a second layer. |
| `install_whisper_model` | Installs a Whisper speech-recognition model file. |
| `install_embedding_model_file` / `install_embedding_tokenizer_file` | Installs the embedding model/tokenizer pair. |
| `create_operator_account` (after the first account) | Adding a new operator/Admin is itself an administrative act. |

Every other command - the actual live-service workflow (search Scripture,
display a verse, approve or reject a suggestion, start/pause/resume a
service, everything Sermon/Music/Content Intelligence does) - is
available to any logged-in operator, Admin or Operator. That is the
concrete shape of "a Sunday volunteer can run the service; only the tech
lead configures it."

## What this is - and isn't - a security boundary against

The real enforcement is `access::ensure_admin`, called at the top of
each gated Rust command body - fail-closed: no session, or a session
that isn't Admin, is refused. The `LoginScreen` itself is a workflow
control ("who is operating right now"), not an attempt to defend against
a determined attacker who could invoke Tauri commands directly outside
this app's own UI; the seven `ensure_admin` calls above are what would
stop that, not the login screen. See `docs/phase-10-audit.md`'s Known
Limitations for what this phase explicitly does not attempt (password
recovery, account editing/deactivation, per-action audit attribution).

## Cross-references

- [`docs/phase-10-audit.md`](phase-10-audit.md) - the full design
  record and every deliberate trade-off's rationale.
- [`docs/phase-10-church-user-roles-permissions.md`](phase-10-church-user-roles-permissions.md) -
  the phase report (what was built, tests, regression, Windows proof).
- [`docs/phase-4-master-plan-gap-audit.md`](phase-4-master-plan-gap-audit.md) -
  names this gap ("Church/user roles & permissions") in the master
  plan's own cross-cutting audit.
- [`docs/bible-translation-registry.md`](bible-translation-registry.md) -
  `generate_verse_embeddings`'s other gate (`ai_processing_allowed`,
  Phase 9), which this phase's Admin check now sits alongside as a
  second, independent layer on the same command.
