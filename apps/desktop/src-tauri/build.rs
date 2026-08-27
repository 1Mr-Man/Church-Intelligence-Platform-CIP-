fn main() {
    // Phase 3.3: embed the git commit this binary was built from, purely
    // for the pilot-diagnostics build identifier (`get_pilot_diagnostics`)
    // and release accountability - a build-time-only `git` invocation
    // (never at runtime, so this adds no runtime exec surface). Falls
    // back to "unknown" rather than failing the build when `git` isn't
    // available or this isn't a git checkout (e.g. built from a source
    // tarball) - a missing build identifier must never break packaging.
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

    tauri_build::build()
}
