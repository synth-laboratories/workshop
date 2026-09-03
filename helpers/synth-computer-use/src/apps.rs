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
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunningApp {
    pub id: String,
    pub display_name: String,
    /// Every main-bundle process for this identifier. More than one is a
    /// targeting error: the caller must `launch` explicitly rather than
    /// guessing which copy to drive.
    pub pids: Vec<i32>,
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
        seen.entry(id.clone())
            .and_modify(|app| {
                if !app.pids.contains(&pid) {
                    app.pids.push(pid);
                }
            })
            .or_insert(RunningApp {
                id,
                display_name: display_name(&bundle),
                pids: vec![pid],
                is_running: true,
            });
    }
    let mut apps: Vec<RunningApp> = seen.into_values().collect();
    for app in &mut apps {
        app.pids.sort_unstable();
    }
    apps.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(apps)
}

/// Every pid currently running as `bundle_id`'s main executable.
pub fn pids_for(bundle_id: &str) -> Vec<i32> {
    running_apps()
        .ok()
        .and_then(|apps| {
            apps.into_iter()
                .find(|app| app.id.eq_ignore_ascii_case(bundle_id))
                .map(|app| app.pids)
        })
        .unwrap_or_default()
}

/// Resolve a bundle identifier to exactly one pid. Never launches.
///
/// Zero copies: the caller must `launch`. More than one: targeting would be a
/// guess, so this names every pid and refuses.
pub fn resolve(bundle_id: &str) -> Result<i32> {
    let pids = pids_for(bundle_id);
    match pids.as_slice() {
        [] => bail!("`{bundle_id}` is not running; call launch"),
        [pid] => Ok(*pid),
        _ => bail!("ambiguous_target{{pids={pids:?}}}"),
    }
}

/// Launch a new copy of `bundle_id` without bringing it forward, and return
/// the pid that did not exist before the call.
///
/// `open -n -g` is load-bearing: without `-n` LaunchServices reuses a running
/// copy, and without `-g` the new copy steals focus (G3).
pub fn launch(bundle_id: &str) -> Result<i32> {
    let before: HashSet<i32> = pids_for(bundle_id).into_iter().collect();
    let status = Command::new("/usr/bin/open")
        .args(["-n", "-g", "-b", bundle_id])
        .status()
        .context("launch app")?;
    if !status.success() {
        bail!("could not launch `{bundle_id}`; is it installed?");
    }
    for _ in 0..50 {
        let created: Vec<i32> = pids_for(bundle_id)
            .into_iter()
            .filter(|pid| !before.contains(pid))
            .collect();
        if created.len() == 1 {
            return Ok(created[0]);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    bail!("`{bundle_id}` did not start a new process within 5 seconds")
}

/// When the target is a Workshop instance that publishes `/health`, mutating
/// verbs must be driving that exact process. Non-Workshop apps have no
/// descriptor and skip the check.
pub fn verify_workshop_target(pid: i32, bundle_id: &str) -> Result<()> {
    let Some(executable) = executable_path(pid) else {
        bail!("`{bundle_id}` pid {pid} is gone");
    };
    let Some(bundle) = bundle_root(&executable) else {
        return Ok(());
    };
    let descriptor_path = bundle.join("Contents/Resources/instance.json");
    let Ok(raw) = std::fs::read(&descriptor_path) else {
        return Ok(());
    };
    let descriptor: Value = serde_json::from_slice(&raw)
        .with_context(|| format!("parse {}", descriptor_path.display()))?;
    if descriptor
        .get("schemaVersion")
        .and_then(Value::as_str)
        != Some("synth.desktop.instance-descriptor.v1")
    {
        return Ok(());
    }
    let Some(data_root) = descriptor.get("data_root").and_then(Value::as_str) else {
        return Ok(());
    };
    let instance_id = descriptor
        .get("instance_id")
        .and_then(Value::as_str)
        .unwrap_or("");
    let driver_path = Path::new(data_root).join("eval-driver.json");
    if !driver_path.is_file() {
        return Ok(());
    }
    let connection: Value = serde_json::from_slice(&std::fs::read(&driver_path)?)
        .with_context(|| format!("parse {}", driver_path.display()))?;
    let url = connection
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim_end_matches('/');
    let token = connection.get("token").and_then(Value::as_str).unwrap_or("");
    if url.is_empty() {
        bail!("target_health_unreachable: eval-driver.json has no url");
    }
    let health = health_get(url, token).with_context(|| {
        format!("target_health_unreachable pid={pid} instance_id={instance_id}")
    })?;
    let health_pid = health
        .pointer("/instance/processId")
        .and_then(Value::as_u64)
        .unwrap_or(0) as i32;
    let health_instance = health
        .pointer("/instance/name")
        .and_then(Value::as_str)
        .unwrap_or("");
    if health_pid != pid || (!instance_id.is_empty() && health_instance != instance_id) {
        bail!(
            "target_identity_mismatch pid={pid} health_pid={health_pid} \
             instance_id={instance_id:?} health_instance={health_instance:?}"
        );
    }
    Ok(())
}

fn health_get(base_url: &str, token: &str) -> Result<Value> {
    let addr = base_url
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or_default()
        .parse::<std::net::SocketAddr>()
        .context("eval-driver url")?;
    let mut stream =
        TcpStream::connect_timeout(&addr, Duration::from_millis(400)).context("connect /health")?;
    stream.set_read_timeout(Some(Duration::from_millis(400)))?;
    stream.set_write_timeout(Some(Duration::from_millis(400)))?;
    let request = format!(
        "GET /health HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .unwrap_or("");
    serde_json::from_str(body).context("decode /health")
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
        assert!(apps
            .iter()
            .all(|app| !app.pids.is_empty() && app.pids.iter().all(|pid| *pid > 0)));
        // The window server or Finder is always running on a live macOS session.
        assert!(!apps.is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn two_copies_of_one_bundle_are_an_ambiguous_target() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let dir = std::env::temp_dir().join(format!(
            "synth-cua-ambiguous-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bundle = dir.join("Fixture.app");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        let exe = bundle.join("Contents/MacOS/Fixture");
        fs::copy("/bin/sleep", &exe).unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        let bundle_id = format!(
            "ai.usesynth.workshop.test.ambiguous-{}",
            std::process::id()
        );
        fs::write(
            bundle.join("Contents/Info.plist"),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>Fixture</string>
<key>CFBundleIdentifier</key><string>{bundle_id}</string>
<key>CFBundleName</key><string>Fixture</string>
<key>CFBundlePackageType</key><string>APPL</string>
</dict></plist>
"#
            ),
        )
        .unwrap();

        let mut first = Command::new(&exe).arg("30").spawn().unwrap();
        let mut second = Command::new(&exe).arg("30").spawn().unwrap();
        let pids = [first.id() as i32, second.id() as i32];
        let mut message = String::new();
        for _ in 0..50 {
            match resolve(&bundle_id) {
                Err(error) => {
                    message = error.to_string();
                    if message.contains("ambiguous_target") {
                        break;
                    }
                }
                Ok(_) => {}
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = first.kill();
        let _ = second.kill();
        let _ = first.wait();
        let _ = second.wait();
        let _ = fs::remove_dir_all(&dir);
        assert!(
            message.contains("ambiguous_target"),
            "expected ambiguous_target, got {message:?}"
        );
        assert!(
            pids.iter().all(|pid| message.contains(&pid.to_string())),
            "error should name both pids {pids:?}: {message}"
        );
    }
}
