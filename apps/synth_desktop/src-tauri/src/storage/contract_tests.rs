#[cfg(test)]
mod contract_tests {
    use crate::storage::{AppEvent, EventSource, APP_EVENT_SCHEMA_VERSION};
    use serde::Deserialize;
    use serde_json::Value;
    use std::{fs, path::PathBuf};

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct AppEventFixture {
        events: Vec<AppEvent>,
        next_sequence: i64,
    }

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct VisualFixture {
        visual: Value,
        revision: Value,
    }

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../packages/runtime-protocol/fixtures")
    }

    #[test]
    fn app_event_fixture_round_trips() {
        let raw = fs::read_to_string(fixtures_dir().join("app-events.json")).unwrap();
        let fixture: AppEventFixture = serde_json::from_str(&raw).unwrap();
        assert_eq!(fixture.next_sequence, 4);
        assert_eq!(fixture.events.len(), 4);
        assert_eq!(fixture.events[0].schema_version, APP_EVENT_SCHEMA_VERSION);
        assert_eq!(fixture.events[0].source, EventSource::Codex);
        assert_eq!(fixture.events[0].kind, "session.started");
        assert_eq!(fixture.events[2].source, EventSource::Visual);
        assert!(fixture.events[3].session_id.is_none());
        let encoded = serde_json::to_value(&fixture.events[0]).unwrap();
        assert_eq!(encoded["schemaVersion"], APP_EVENT_SCHEMA_VERSION);
        assert_eq!(encoded["eventId"], "evt_codex_session_started");
        assert_eq!(encoded["source"], "codex");
    }

    #[test]
    fn visual_record_fixture_has_required_fields() {
        let raw = fs::read_to_string(fixtures_dir().join("visual-record.json")).unwrap();
        let fixture: VisualFixture = serde_json::from_str(&raw).unwrap();
        assert_eq!(fixture.visual["schemaVersion"], "synth.desktop-visual.v1");
        assert_eq!(fixture.visual["id"], "vis_reward_1");
        assert_eq!(fixture.visual["status"], "saved");
        assert_eq!(fixture.revision["revision"], 1);
        assert_eq!(fixture.revision["visualId"], "vis_reward_1");
    }
}
