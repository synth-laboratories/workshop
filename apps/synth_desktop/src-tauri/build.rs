fn main() {
    println!("cargo:rerun-if-env-changed=SYNTH_DESKTOP_SOURCE_REVISION");
    track_git_revision_inputs();
    let revision = std::env::var("SYNTH_DESKTOP_SOURCE_REVISION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "--short=12", "HEAD"])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "unknown".into());
    let built_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs().to_string())
        .unwrap_or_else(|_| "0".into());
    println!("cargo:rustc-env=SYNTH_BUILD_REVISION={revision}");
    println!("cargo:rustc-env=SYNTH_BUILD_TIMESTAMP={built_at}");
    tauri_build::build()
}

/// Cargo does not know that the `git rev-parse` fallback depends on repository
/// metadata. Without these directives, a build launched outside the instance
/// wrapper can keep an old embedded revision until some Rust source happens to
/// change. `git rev-parse --git-path` also resolves linked-worktree metadata to
/// the authoritative common Git directory.
fn track_git_revision_inputs() {
    track_git_path("HEAD");
    let Some(reference) = git_stdout(&["symbolic-ref", "-q", "HEAD"]) else {
        return;
    };
    track_git_path(&reference);
    // A packed ref remains authoritative when the loose branch ref is absent.
    track_git_path("packed-refs");
}

fn track_git_path(path: &str) {
    if let Some(path) = git_stdout(&["rev-parse", "--git-path", path]) {
        println!("cargo:rerun-if-changed={path}");
    }
}

fn git_stdout(args: &[&str]) -> Option<String> {
    std::process::Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}
