# Phase 10 Audit — Church/User Roles & Permissions

## Trigger

The user's own instruction: "Keep going into church/user roles & permissions
next." This is item 5 in `docs/phase-4-master-plan-gap-audit.md`'s own
"Proposed Phase 4 candidates" list (items 1/2/6 already delivered as
Phase 4.4, Phase 7.1-7.3, and Phase 8 respectively): **"Church/user roles
& permissions — the prerequisite for any real multi-user or multi-church
deployment."** That same document's cross-cutting table grades it
`NOT STARTED`: *"No multi-user model exists at all - the app has no
login, no user table, no role enforcement. Single-operator, single-machine
only."*

A note on a second part of the user's message: it also pasted a list of
items numbered 9-12 ("multi-language Whisper," "cross-sermon analytics,"
"congregant companion view," "real audio fingerprinting") under a header
calling it "Phase 8 candidate." Those numbers come from the external
advice document's own gap list, not this project's actual phase history
- item 12 ("real audio fingerprinting") in particular is **already
shipped** here (Phase 7.1 real fingerprinting algorithm, Phase 7.2
enrollment workflow, Phase 7.3 remove-enrollment - all with direct
binary/Windows verification). The user's own Phase 8 and Phase 9 in
*this* codebase (Production Integration; Bible Translation Registry v2)
are both already complete, committed, pushed, and Windows-verified - nothing
was skipped. This phase proceeds on the clear, unambiguous instruction:
roles & permissions, next.

## What "roles & permissions" means for THIS product

CIP is a single local desktop application, one install per church, no
cloud, no network dependency (spec section 33/34, reaffirmed every phase
since). It is not a multi-tenant SaaS product. So "multi-user" here does
not mean separate church organizations sharing a server - it means
**separate human operators of the same physical installation**, each
service or season, who should not have identical access: a pastor or the
church's tech lead configures licensing, OBS/vMix credentials, and AI
model files once; a rotating cast of Sunday volunteers runs the actual
live service (search, display, approve/reject AI suggestions) without
ever touching those settings, whether by mistake or by not knowing they
exist.

## Design choices (no genuine architectural fork; proceeding directly)

**1. Two roles, not a general RBAC framework.** `Admin` and `Operator` -
a closed enum, matching this codebase's established convention for
closed, non-`#[non_exhaustive]` enums that model exactly what's needed
now (`ContentType`, `LicensingStatus`). A third role (e.g. a read-only
"Viewer") is a reasonable future addition, deliberately not designed
here - nothing in this phase's scope needs it yet, and inventing it
without a real caller would be exactly the premature abstraction this
project's own discipline forbids.

**2. Local, PIN-based authentication - not a general auth system.** The
threat model is a person physically at this one desktop machine, not a
remote or networked attacker (the same reasoning this project's own
security audit already applied: *"no encrypted credential storage - moot
today since there are no external API keys in the app"*). Accounts are
identified by a short PIN (minimum 4 characters), hashed as
`base64(sha256(salt || pin))` with a random per-account salt - the exact
algorithm shape `integrations/obs`'s `compute_auth_response` already
uses for obs-websocket auth (Phase 8), reused here rather than adding a
new dependency (`bcrypt`/`argon2`) for a threat model that doesn't need
their offline-brute-force resistance. This is a deliberate, honestly-
documented trade-off, not an oversight - stated plainly in
`docs/roles-permissions.md` and this phase's known limitations.

**3. Session-only login, no "remember me."** `AppState.current_operator`
is in-memory only (`Mutex<Option<OperatorSession>>`), never persisted -
identical precedent to `screen_route_modes` (Live/Held routing) and
every other session-scoped `AppState` field: a restart requires logging
in again. This matches how a real live-service operator app should
behave (a different volunteer may run the next service) and avoids
adding any new persistence-security surface for a "logged in" flag.

**4. Bootstrap: the first account created is always Admin, unconditionally.**
With zero accounts in `operator_accounts`, `create_operator_account`
accepts a name+PIN with no login required and makes it Admin regardless
of the role requested - there is no other way for the very first account
to ever become an Admin. Once at least one account exists, creating
another requires the caller to already be a logged-in Admin. This mirrors
the "no operators exist yet" bootstrap problem every real access-control
system has and resolves it the same way most do: the first setup step is
inherently unauthenticated, everything after it is not.

**5. Gate real commands, not a generic middleware layer.** Following the
`ensure_ai_processing_permitted`/`ensure_translation_selectable` precedent
(a pure, directly-testable gate function called at the top of specific
command bodies, not a framework), this phase adds `access::ensure_admin`
and calls it at the start of exactly the commands that already are
"configuration, not day-to-day operation": `import_bible_dataset`,
`set_content_enabled`, `set_production_integration_config`,
`generate_verse_embeddings`, `install_whisper_model`,
`install_embedding_model_file`, `install_embedding_tokenizer_file`, and
(after bootstrap) `create_operator_account` itself. Every other command -
search, display, approve/reject, start/pause/resume a service, Sermon/
Music/Content Intelligence, Production Integration's own push (once
configured) - remains available to any logged-in operator, Admin or not.
This is the actual, concrete shape of "a Sunday volunteer can run the
service; only the tech lead configures it."

**6. The app requires *someone* logged in to do anything at all** (the
literal gap: *"the app has no login"*) - the frontend gates the entire
workspace behind a login screen, mirroring `WebRuntimeNotice`'s existing
top-level `App.tsx` gate pattern exactly. This is enforced honestly only
at the frontend today (see Known Limitations) - see the Testing Boundary
section for why the *sensitive-command* gate, not the *screen* gate, is
what carries the real security-relevant guarantee.

## Testing boundary

Every backend gate is enforced in Rust regardless of what the frontend
renders - `ensure_admin`/`create_operator_account`/`login` are pure
functions tested directly against real in-memory SQLite stores (the
established `resolve_default_translation_id_from_registry`/
`ensure_ai_processing_permitted` pattern: no `tauri::test` harness
exists in this project). Pin hashing/verification round-trips are unit
tested in `core/access` directly. The Tauri command wrappers that call
these gates are thin orchestration, per this project's own established
no-redundant-command-level-tests discipline.

## What this phase explicitly does NOT do (honest, not silent)

- No password recovery/reset flow - an Admin locked out has no in-app
  recovery path this phase; a future phase could add a recovery
  mechanism (e.g. a special first-run reset), not attempted here.
- No account editing/deactivation/deletion - `list`/`create` only. An
  operator who has left cannot be removed from the account list today.
- No per-command audit trail of *who* (which operator) performed an
  action - `audit_events`/timeline already record *what* happened; they
  do not yet record *which operator* triggered it. A real, separate,
  future addition (would need `current_operator` threaded into every
  existing write path), not attempted here to keep this phase's diff
  focused on the access-control mechanism itself.
- The frontend's login gate is a UX/workflow control ("who is
  operating"), not itself a security boundary - Tauri's own IPC surface
  has no notion of "logged in" separate from `AppState`, so the *real*
  boundary is the seven `ensure_admin` calls inside the Rust commands
  themselves; a determined operator who could otherwise invoke Tauri
  commands directly (outside this app's own UI) would still be stopped
  by those, but the login *screen* itself is not attempting to defend
  against that class of attacker - honest about what "the app requires
  login" does and doesn't guarantee.
- No multi-church/multi-tenant data isolation - this remains one SQLite
  database per install, exactly as before; "multi-user" here means
  multiple human operators of one installation, not multiple churches
  sharing one instance.
