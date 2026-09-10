//! Launch the same packaged runtime composition used by Desktop, with no
//! WebViews. The existing instance lock remains the sole ownership authority.
use super::transport::RuntimeClient;
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

pub fn start(client: &RuntimeClient, executable: &Path, root: &Path) -> Result<Value> {
    if let Ok(description) = client.describe() {
        return Ok(description);
    }
    let native = executable
        .parent()
        .context("missing executable directory")?
        .join(if cfg!(windows) {
            "synth-desktop.exe"
        } else {
            "synth-desktop"
        });
    anyhow::ensure!(
        native.is_file(),
        "runtime executable missing beside Workshop CLI; build/install Desktop first"
    );
    fs::create_dir_all(root)?;
    let mut options = fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let log_path = root.join("runtime-start.log");
    let log = options.open(&log_path)?;
    let mut command = Command::new(native);
    command
        .args(["--workshop-runtime", "--workshop-data-root"])
        .arg(root)
        .env("SYNTH_DESKTOP_DATA_ROOT", root)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().context("start Workshop runtime")?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(description) = client.describe() {
            return Ok(description);
        }
        if let Some(status) = child.try_wait()? {
            anyhow::bail!("runtime exited ({status}); inspect {}", log_path.display());
        }
        if Instant::now() >= deadline {
            // Do not kill a process whose initialization outcome is uncertain.
            return Ok(
                json!({"starting": true, "processId": child.id(), "log": log_path,
                "message":"Runtime startup has not completed. Run workshop doctor for readiness."}),
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
