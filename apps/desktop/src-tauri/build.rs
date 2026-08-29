fn main() {
    // Phase 3.3: embed the git commit this binary was built from, purely
    // for the pilot-diagnostics build identifier (`get_pilot_diagnostics`)
    // and release accountability - a build-time-only `git` invocation
    // (never at runtime, so this adds no runtime exec surface). Falls
    // back to "unknown" rather than failing the build when `git` isn't
    // available or this isn't a git checkout (e.g. built from a source
    // tarball) - a missing build identifier must never break packaging.
    //
    // Phase 3.8.7: without an explicit `cargo:rerun-if-changed` directive
    // here, Cargo's *default* rerun heuristic only tracks files inside
    // this crate's own package directory - never `.git/`, which lives at
    // the workspace root. In this project's own build-then-commit
    // workflow (rebuild the Windows artifact, verify it, *then* `git
    // commit`), that meant a build run between two commits with no other
    // `apps/desktop/src-tauri` file changed (exactly Phase 3.8.6.1: no
    // Rust/TS source changed, only release artifacts/config/docs outside
    // this crate) would never rerun this build script at all, silently
    // keeping whatever commit hash was embedded by the *previous* build -
    // this is the concrete, confirmed root cause of a real Windows
    // installer showing a build identifier two phases stale. Explicitly
    // watching `.git/HEAD` and the ref it points to (resolved via `git
    // rev-parse --git-dir`, never a hardcoded relative path, so this
    // keeps working from a worktree or a differently-nested checkout)
    // makes every real commit change force a rerun.
    let git_dir = std::process::Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string());
    if let Some(git_dir) = &git_dir {
        let head_path = format!("{git_dir}/HEAD");
        println!("cargo:rerun-if-changed={head_path}");
        if let Ok(head_contents) = std::fs::read_to_string(&head_path) {
            if let Some(ref_path) = head_contents.trim().strip_prefix("ref: ") {
                println!("cargo:rerun-if-changed={git_dir}/{ref_path}");
            }
        }
    }

    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=CIP_GIT_COMMIT={commit}");

    // Phase 3.8.7: this project's own workflow always builds and verifies
    // an artifact *before* committing the very changes that produced it -
    // meaning `commit` above is, by construction, routinely one phase
    // behind a freshly built binary. Surfacing whether the working tree
    // had uncommitted changes at build time (rather than only the last
    // real commit hash) lets a diagnostics reader tell "this is exactly
    // commit X" apart from "this was built from X plus uncommitted work",
    // instead of silently trusting a commit hash that may already be
    // stale relative to the binary in hand.
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    println!("cargo:rustc-env=CIP_GIT_DIRTY={dirty}");

    tauri_build::build()
}
