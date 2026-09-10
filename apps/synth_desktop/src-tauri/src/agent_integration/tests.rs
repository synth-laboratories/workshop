use super::*;
use std::fs;

#[test]
fn native_caller_scope_survives_unified_bridge_without_borrowing_a_session() {
    let tools = json!({"tools":[
        {"name":"optimizer_list_recipes","inputSchema":{"properties":{"session_ref":{"type":"string"}}}},
        {"name":"session_get","inputSchema":{"properties":{"session_id":{"type":"string"}}}}
    ]});
    assert_eq!(bind_native_caller_session(&tools, "optimizer_list_recipes", &json!({}), Some("native-chat")).unwrap(), json!({"session_ref":"native-chat"}));
    assert!(bind_native_caller_session(&tools, "optimizer_list_recipes", &json!({"session_ref":"other-chat"}), Some("native-chat")).is_err());
    assert_eq!(bind_native_caller_session(&tools, "optimizer_list_recipes", &json!({}), None).unwrap(), json!({}));
    let target = json!({"session_id":"explicit-target"});
    assert_eq!(bind_native_caller_session(&tools, "session_get", &target, Some("native-chat")).unwrap(), target);
}

#[test]
fn native_caller_scope_binds_declared_nested_operation_arguments() {
    let tools = json!({"tools":[{"name":"optimizer_manage","inputSchema":{
        "x-workshop-caller-session-path":"/arguments/session_ref"
    }}]});
    let args = json!({"operation":"list_recipes","arguments":{}});
    let bound = bind_native_caller_session(&tools, "optimizer_manage", &args, Some("native-chat")).unwrap();
    assert_eq!(bound["arguments"]["session_ref"], "native-chat");
    assert_eq!(bound["operation"], "list_recipes");
    assert!(bound.get("session_ref").is_none());
    let wrong = json!({"operation":"list_recipes","arguments":{"session_ref":"another-chat"}});
    assert!(bind_native_caller_session(&tools, "optimizer_manage", &wrong, Some("native-chat")).is_err());
    let wrong_alias = json!({"operation":"evaluation_start","arguments":{"sessionRef":"another-chat"}});
    assert!(bind_native_caller_session(&tools, "optimizer_manage", &wrong_alias, Some("native-chat")).is_err());
    assert_eq!(bind_native_caller_session(&tools, "optimizer_manage", &args, None).unwrap(), args);
}

fn temp() -> tempfile::TempDir {
    tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap()
}

#[test]
fn configuration_round_trip_preserves_unrelated_settings_and_refuses_changed_ownership() {
    for host in ["codex", "claude"] {
        let dir = temp();
        let path = dir.path().join(if host == "codex" {
            "config.toml"
        } else {
            ".claude.json"
        });
        let original = if host == "codex" {
            "# keep this comment\nmodel = 'example'\n[mcp_servers.other]\ncommand = 'other-server'\n"
        } else {
            "{\"theme\":\"dark\",\"mcpServers\":{\"other\":{\"command\":\"other-server\"}},\"projects\":{\"/my-project\":{}}}\n"
        };
        fs::write(&path, original).unwrap();
        let exe = dir.path().join("Workshop app/Contents/MacOS/workshop");
        let root = dir.path().join("instance with spaces");
        config::configure(host, dir.path(), &exe, &root, true).unwrap();
        let once = fs::read_to_string(&path).unwrap();
        config::configure(host, dir.path(), &exe, &root, true).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            once,
            "connect must be idempotent"
        );
        assert!(config::configure(host, dir.path(), &exe, &root.join("other"), true).is_err());
        assert!(config::configure(host, dir.path(), &exe.join("different"), &root, false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), once);
        config::configure(host, dir.path(), &exe, &root, false).unwrap();
        let after = fs::read_to_string(&path).unwrap();
        if host == "codex" {
            assert!(after.contains("# keep this comment"));
            let before: toml::Value = toml::from_str(original).unwrap();
            let after: toml::Value = toml::from_str(&after).unwrap();
            assert_eq!(before, after);
        } else {
            assert_eq!(
                serde_json::from_str::<Value>(original).unwrap(),
                serde_json::from_str::<Value>(&after).unwrap()
            );
        }
    }
}

#[test]
fn invalid_config_is_never_replaced() {
    for (host, file) in [("codex", "config.toml"), ("claude", ".claude.json")] {
        let dir = temp();
        fs::write(dir.path().join(file), "invalid {[").unwrap();
        assert!(config::configure(
            host,
            dir.path(),
            Path::new("/workshop"),
            Path::new("/instance"),
            true
        )
        .is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join(file)).unwrap(),
            "invalid {["
        );
    }
}

#[test]
fn explicit_scope_is_required_and_plugins_share_one_skill_and_server() {
    assert!(parse(vec!["mcp".into()]).is_err());
    assert!(parse(vec!["mcp".into(), "--data-root".into(), "relative".into()]).is_err());
    let dir = temp();
    let output = dir.path().join("workshop");
    export_plugin(
        &output,
        Path::new("/Workshop app/workshop"),
        Path::new("/instance"),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(output.join("skills/workshop/SKILL.md")).unwrap(),
        SKILL
    );
    let mcp: Value = serde_json::from_slice(&fs::read(output.join(".mcp.json")).unwrap()).unwrap();
    assert_eq!(
        mcp["mcpServers"]["workshop"],
        server_config(Path::new("/Workshop app/workshop"), Path::new("/instance"))
    );
    assert!(output.join(".codex-plugin/plugin.json").exists());
    assert!(output.join(".claude-plugin/plugin.json").exists());
    assert!(export_plugin(&output, Path::new("/workshop"), Path::new("/instance")).is_err());
}

#[test]
fn stopped_runtime_fails_without_reconfiguring_a_client() {
    let dir = temp();
    let config_root = dir.path().join("client");
    assert!(run(vec![
        "connect".into(),
        "codex".into(),
        "--data-root".into(),
        dir.path().display().to_string(),
        "--config-root".into(),
        config_root.display().to_string()
    ])
    .is_err());
    assert!(!config_root.exists());
}

#[cfg(unix)]
#[test]
fn descriptor_cannot_redirect_credentials_off_machine_or_through_a_symlink() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = temp();
    let path = dir.path().join("visuals-ipc.json");
    fs::write(
        &path,
        r#"{"url":"http://192.0.2.1:9000","token":"test-only-sentinel"}"#,
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let client = RuntimeClient::new(dir.path().to_path_buf()).unwrap();
    let error = client.describe().unwrap_err().to_string();
    assert!(error.contains("loopback"));
    assert!(!error.contains("test-only-sentinel"));
    fs::rename(&path, dir.path().join("other.json")).unwrap();
    symlink(dir.path().join("other.json"), &path).unwrap();
    assert!(client
        .describe()
        .unwrap_err()
        .to_string()
        .contains("private file"));
}

#[test]
fn descriptor_replacement_refreshes_retry_scope_without_exposing_credentials() {
    let root = temp();
    let client = super::transport::RuntimeClient::new(root.path().to_path_buf()).unwrap();
    let arguments = serde_json::json!({"visual_id":"test"});
    let stopped = client.breaker_arguments(&arguments);
    let path = root.path().join("visuals-ipc.json");
    std::fs::write(&path, "private-test-marker").unwrap();
    let running = client.breaker_arguments(&arguments);
    assert_ne!(stopped, running);
    assert!(!running.to_string().contains("private-test-marker"));
    std::fs::write(&path, "replacement-marker-with-a-different-length").unwrap();
    assert_ne!(running, client.breaker_arguments(&arguments));
    assert_eq!(arguments, serde_json::json!({"visual_id":"test"}));
}

#[test]
fn operation_call_requires_explicit_instance_and_absolute_argument_file() {
    assert!(parse(vec!["call".into(), "optimizers_list".into()]).is_err());
    assert!(parse(vec!["call".into(), "optimizers_list".into(), "--data-root".into(), "/tmp/instance".into(), "--arguments-file".into(), "relative.json".into()]).is_err());
    let args = parse(vec!["call".into(), "optimizers_list".into(), "--data-root".into(), "/tmp/instance".into(), "--arguments-file".into(), "/tmp/arguments.json".into()]).unwrap();
    assert_eq!(args.operation.as_deref(), Some("optimizers_list"));
    assert_eq!(args.arguments_file.as_deref(), Some(Path::new("/tmp/arguments.json")));
}
