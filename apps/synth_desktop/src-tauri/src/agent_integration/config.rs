//! Preserve unrelated host settings and refuse to overwrite an existing
//! Workshop entry with different ownership, arguments, or policy.

use anyhow::{Context, Result};
use fs2::FileExt;
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub fn configure(
    host: &str,
    config_root: &Path,
    executable: &Path,
    root: &Path,
    connect: bool,
) -> Result<PathBuf> {
    anyhow::ensure!(
        config_root.is_absolute(),
        "configuration root must be absolute"
    );
    fs::create_dir_all(config_root)?;
    let path = config_root.join(if host == "codex" {
        "config.toml"
    } else {
        ".claude.json"
    });
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(config_root.join(".workshop-connect.lock"))?;
    lock.try_lock_exclusive()
        .context("another Workshop setup is changing this configuration")?;
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        anyhow::ensure!(
            !metadata.file_type().is_symlink(),
            "configuration is a symlink; configure its actual location explicitly"
        );
    }
    let original = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let expected = super::server_config(executable, root);
    let updated = if host == "codex" {
        edit_codex(&original, &expected, connect)?
    } else {
        edit_claude(&original, &expected, connect)?
    };
    if updated == original {
        return Ok(path);
    }
    // Do not truncate a live config. Stage privately in the same directory,
    // then replace it only if a non-cooperating editor has not changed it.
    let stage = config_root.join(format!(".workshop-config-{}", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&stage)?;
        file.write_all(updated.as_bytes())?;
        file.sync_all()?;
        let current = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error.into()),
        };
        anyhow::ensure!(
            current == original,
            "client configuration changed during setup; retry"
        );
        fs::rename(&stage, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(stage);
    }
    result?;
    Ok(path)
}

fn check_existing(existing: Option<&Value>, expected: &Value) -> Result<()> {
    if let Some(existing) = existing {
        anyhow::ensure!(existing == expected,
            "a different or customized Workshop entry already exists; preserve it and resolve the conflict before setup or removal");
    }
    Ok(())
}

fn edit_codex(original: &str, expected: &Value, connect: bool) -> Result<String> {
    let parsed: toml::Value =
        toml::from_str(original).context("invalid Codex TOML; left unchanged")?;
    let existing = parsed
        .get("mcp_servers")
        .and_then(|servers| servers.get("workshop"))
        .map(serde_json::to_value)
        .transpose()?;
    check_existing(existing.as_ref(), expected)?;
    if connect && existing.is_some() || !connect && existing.is_none() {
        return Ok(original.into());
    }
    let mut document = original.parse::<toml_edit::Document>()?;
    if connect {
        if document.get("mcp_servers").is_none() {
            document["mcp_servers"] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        let servers = document["mcp_servers"]
            .as_table_like_mut()
            .context("mcp_servers must be a table")?;
        let mut server = toml_edit::Table::new();
        server["command"] = toml_edit::value(
            expected["command"]
                .as_str()
                .context("invalid executable path")?,
        );
        let mut args = toml_edit::Array::new();
        for arg in expected["args"].as_array().unwrap() {
            args.push(arg.as_str().context("invalid argument path")?);
        }
        server["args"] = toml_edit::value(args);
        servers.insert("workshop", toml_edit::Item::Table(server));
    } else {
        document["mcp_servers"]
            .as_table_like_mut()
            .context("mcp_servers must be a table")?
            .remove("workshop");
    }
    Ok(document.to_string())
}

fn edit_claude(original: &str, expected: &Value, connect: bool) -> Result<String> {
    let mut document: Value = if original.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(original).context("invalid Claude JSON; left unchanged")?
    };
    let object = document
        .as_object_mut()
        .context("Claude config must be an object")?;
    if let Some(servers) = object.get("mcpServers") {
        anyhow::ensure!(servers.is_object(), "mcpServers must be an object");
    }
    let existing = object
        .get("mcpServers")
        .and_then(|servers| servers.get("workshop"));
    check_existing(existing, expected)?;
    if connect && existing.is_some() || !connect && existing.is_none() {
        return Ok(original.into());
    }
    let servers = object
        .entry("mcpServers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .unwrap();
    if connect {
        servers.insert("workshop".into(), expected.clone());
    } else {
        servers.remove("workshop");
    }
    Ok(format!("{}\n", serde_json::to_string_pretty(&document)?))
}
