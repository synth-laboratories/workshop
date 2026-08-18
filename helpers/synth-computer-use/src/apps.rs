//! Mapping between bundle identifiers and running processes.
//!
//! Everything above this layer speaks bundle identifiers, because a pid is not
//! a stable name for anything — the allowlist has to survive an app restart, and
//! "com.apple.mail" does while "pid 4213" does not.
//!
//! Resolution goes through `libproc` and the bundle's own `Info.plist` rather
//! than AppKit. `NSRunningApplication` would mean linking AppKit and running an
//! Objective-C runtime inside a helper whose whole job is to be small and
//! auditable, and `proc_pidpath` answers the same question.

use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunningApp {
    pub id: String,
    pub display_name: String,
    pub pid: i32,
    pub is_running: bool,
}

extern "C" {
    fn proc_listallpids(buffer: *mut libc::c_void, buffersize: libc::c_int) -> libc::c_int;
    fn proc_pidpath(
        pid: libc::c_int,
        buffer: *mut libc::c_void,
        buffersize: u32,
    ) -> libc::c_int;
}

const PROC_PIDPATHINFO_MAXSIZE: usize = 4 * libc::PATH_MAX as usize;

/// Executable path for a pid, or `None` when the process is gone or not ours
/// to inspect.
pub fn executable_path(pid: i32) -> Option<PathBuf> {
    let mut buffer = vec![0u8; PROC_PIDPATHINFO_MAXSIZE];
    let written = unsafe {
        proc_pidpath(
            pid,
            buffer.as_mut_ptr() as *mut libc::c_void,
            buffer.len() as u32,
        )
    };
    if written <= 0 {
        return None;
    }
    buffer.truncate(written as usize);
    Some(PathBuf::from(String::from_utf8_lossy(&buffer).into_owned()))
}

/// Walk up from `…/Foo.app/Contents/MacOS/Foo` to `…/Foo.app`.
pub fn bundle_root(executable: &Path) -> Option<PathBuf> {
    let mut current = executable;
    while let Some(parent) = current.parent() {
        if parent.extension().and_then(|ext| ext.to_str()) == Some("app") {
            return Some(parent.to_path_buf());
        }
        current = parent;
    }
    None
}

/// Bundle identifiers are cached: `plutil` is a subprocess, and enumerating
/// every process would otherwise fork a few hundred times per `list_apps`.
static BUNDLE_CACHE: Mutex<Option<HashMap<PathBuf, Option<String>>>> = Mutex::new(None);

/// Read `CFBundleIdentifier` out of a bundle's `Info.plist`.
///
/// Shells out to `plutil` because these plists are usually the binary format,
/// and shipping a plist parser to read one string would be a lot of surface for
/// no benefit.
pub fn bundle_identifier(bundle: &Path) -> Option<String> {
    let mut guard = BUNDLE_CACHE.lock().ok()?;
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(cached) = cache.get(bundle) {
        return cached.clone();
    }
    let plist = bundle.join("Contents/Info.plist");
    let resolved = Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
        .arg(&plist)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty());
    cache.insert(bundle.to_path_buf(), resolved.clone());
    resolved
}

/// Whether this process is the bundle's declared application executable.
///
/// Adapter binaries stored inside an app bundle inherit the outer `.app` when
/// we walk their path upward, but they are not the AX application. Filtering
/// by CFBundleExecutable prevents an orphaned MCP child from being selected as
/// the target merely because process enumeration happened to return it first.
pub fn is_main_bundle_process(bundle: &Path, executable: &Path) -> bool {
    let plist = bundle.join("Contents/Info.plist");
    let Some(name) = Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleExecutable", "raw", "-o", "-"])
        .arg(&plist)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return false;
    };
    executable == bundle.join("Contents/MacOS").join(name)
}

fn display_name(bundle: &Path) -> String {
    bundle
        .file_stem()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Unknown".into())
}

/// Every running app that has a bundle. Command-line processes have no bundle
/// identifier and are not drivable, so they are left out rather than listed
/// with an empty id.
pub fn running_apps() -> Result<Vec<RunningApp>> {
    let count = unsafe { proc_listallpids(std::ptr::null_mut(), 0) };
    if count <= 0 {
        bail!("could not enumerate processes");
    }
    // Headroom: processes start between the sizing call and the read.
    let mut pids = vec![0i32; count as usize * 2];
    let pid_count = unsafe {
        proc_listallpids(
            pids.as_mut_ptr() as *mut libc::c_void,
            (pids.len() * std::mem::size_of::<i32>()) as libc::c_int,
        )
    };
    if pid_count <= 0 {
        bail!("could not enumerate processes");
    }
    // Unlike proc_pidlist, proc_listallpids returns a number of PIDs, not a
    // byte count. Dividing it by sizeof(pid_t) silently discarded three
    // quarters of the process list and could leave only a bundled helper for
    // an app, causing us to attach AX to that helper instead of the app.
    pids.truncate(pid_count as usize);

    let mut seen: HashMap<String, RunningApp> = HashMap::new();
    for pid in pids {
        if pid <= 0 {
            continue;
        }
        let Some(executable) = executable_path(pid) else {
            continue;
        };
        let Some(bundle) = bundle_root(&executable) else {
            continue;
        };
        if !is_main_bundle_process(&bundle, &executable) {
            continue;
        }
        let Some(id) = bundle_identifier(&bundle) else {
            continue;
        };
        seen.entry(id.clone()).or_insert(RunningApp {
            id,
            display_name: display_name(&bundle),
            pid,
            is_running: true,
        });
    }
    let mut apps: Vec<RunningApp> = seen.into_values().collect();
    apps.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(apps)
}

/// Find the pid for a bundle identifier.
pub fn pid_for(bundle_id: &str) -> Option<i32> {
    running_apps()
        .ok()?
        .into_iter()
        .find(|app| app.id.eq_ignore_ascii_case(bundle_id))
        .map(|app| app.pid)
}

/// Launch an app without bringing it forward, then wait for it to register.
///
/// `open -g` is what keeps this background: launching normally would activate
/// the app and steal the operator's focus, which is the exact failure G3 is
/// about.
pub fn launch_in_background(bundle_id: &str) -> Result<i32> {
    let status = Command::new("/usr/bin/open")
        .args(["-g", "-b", bundle_id])
        .status()
        .context("launch app")?;
    if !status.success() {
        bail!("could not launch `{bundle_id}`; is it installed?");
    }
    // Registration is not instant. Polling beats a fixed sleep: most apps
    // appear in well under a second and this returns as soon as they do.
    for _ in 0..50 {
        if let Some(pid) = pid_for(bundle_id) {
            return Ok(pid);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    bail!("`{bundle_id}` did not start within 5 seconds")
}

/// Resolve an app to a pid, launching it in the background if needed.
pub fn resolve_or_launch(bundle_id: &str) -> Result<i32> {
    match pid_for(bundle_id) {
        Some(pid) => Ok(pid),
        None => launch_in_background(bundle_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bundle_root_is_found_from_a_nested_executable() {
        assert_eq!(
            bundle_root(Path::new("/Applications/Mail.app/Contents/MacOS/Mail")),
            Some(PathBuf::from("/Applications/Mail.app"))
        );
        // Nested helpers resolve to the innermost bundle, which is the one
        // whose Info.plist describes the running executable.
        assert_eq!(
            bundle_root(Path::new(
                "/Applications/Foo.app/Contents/Library/Bar.app/Contents/MacOS/Bar"
            )),
            Some(PathBuf::from("/Applications/Foo.app/Contents/Library/Bar.app"))
        );
        assert_eq!(bundle_root(Path::new("/usr/bin/ssh")), None);
    }

    #[test]
    fn a_display_name_comes_from_the_bundle_name() {
        assert_eq!(display_name(Path::new("/Applications/Mail.app")), "Mail");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_current_process_has_a_resolvable_executable_path() {
        let path = executable_path(std::process::id() as i32).unwrap();
        assert!(path.exists(), "{}", path.display());
    }

    /// Command-line processes have no bundle and are not drivable; listing them
    /// with an empty identifier would put unusable entries in front of an agent.
    #[cfg(target_os = "macos")]
    #[test]
    fn every_listed_app_has_a_real_bundle_identifier() {
        let apps = running_apps().unwrap();
        assert!(apps.iter().all(|app| !app.id.is_empty()));
        assert!(apps.iter().all(|app| app.pid > 0));
        // The window server or Finder is always running on a live macOS session.
        assert!(!apps.is_empty());
    }
}
