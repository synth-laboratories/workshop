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
    #[cfg(target_os = "macos")]
    build_ghostty_host();
    tauri_build::build()
}

#[cfg(target_os = "macos")]
fn build_ghostty_host() {
    use std::path::{Path, PathBuf};

    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set"),
    );
    let package = manifest_dir.join("ghostty-host");
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());
    let swift_configuration = if profile == "release" { "release" } else { "debug" };
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set"));
    let target_root = out_dir
        .ancestors()
        .nth(4)
        .expect("OUT_DIR lives below the Cargo target root");
    let scratch = target_root.join("ghostty-host");

    println!("cargo:rerun-if-changed={}", package.join("Package.swift").display());
    println!(
        "cargo:rerun-if-changed={}",
        package.join("Package.resolved").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        package.join("Sources/SynthGhosttyHost/SynthGhosttyHost.swift").display()
    );

    let status = std::process::Command::new("swift")
        .args([
            "build",
            "--package-path",
            package.to_str().expect("Ghostty host package path is UTF-8"),
            "--scratch-path",
            scratch.to_str().expect("Ghostty host build path is UTF-8"),
            "--configuration",
            swift_configuration,
            "--product",
            "SynthGhosttyHost",
        ])
        .status()
        .expect("launch Swift to build the libghostty host");
    assert!(status.success(), "building the libghostty host failed");

    let dylib = find_file(&scratch, "libSynthGhosttyHost.dylib")
        .expect("Swift build did not produce libSynthGhosttyHost.dylib");
    let link_dir = dylib.parent().expect("Ghostty host dylib has a parent");
    let generated = manifest_dir.join("generated-frameworks");
    std::fs::create_dir_all(&generated).expect("create generated framework directory");
    std::fs::copy(&dylib, generated.join("libSynthGhosttyHost.dylib"))
        .expect("stage the Ghostty host for Tauri bundling");
    let executable_dir = target_root.join(&profile);
    std::fs::create_dir_all(&executable_dir).expect("create Cargo profile directory");
    std::fs::copy(&dylib, executable_dir.join("libSynthGhosttyHost.dylib"))
        .expect("stage the Ghostty host beside the development executable");

    println!("cargo:rustc-link-search=native={}", link_dir.display());
    println!("cargo:rustc-link-lib=dylib=SynthGhosttyHost");
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path");
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/..");

    fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
        let entries = std::fs::read_dir(root).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.file_name().and_then(|value| value.to_str()) == Some(name) {
                return Some(path);
            }
            if path.is_dir() {
                if let Some(found) = find_file(&path, name) {
                    return Some(found);
                }
            }
        }
        None
    }
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
