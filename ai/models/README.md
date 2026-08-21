# AI models

Local model artifacts (speech, embeddings, classifiers) are downloaded or
copied into the app's configured model directory at runtime - see
`docs/architecture.md` and `apps/desktop/src-tauri/src/config.rs` for how
that path is resolved (`AppConfig::model_dir`). This directory in the
repository is a placeholder for local development only.

Nothing here is checked into git (see `.gitignore`): model files are large
binary artifacts, not source code, and CIP must not assume a specific model
is bundled or vendored into the repository. Which model backend is used
(e.g. a local Whisper variant for `cip-ai-speech`) is decided per-integration,
not hard-coded into `core`.
