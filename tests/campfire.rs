//! Campfire is pi under a different home. These tests confirm the delegation:
//! it reads/writes the identical format and produces the identical conversion,
//! only stamped as a different harness.

use serde_json::json;
use txcript::{Block, Campfire, CampfireStore, Codec, Harness, Role, Store, Tool};

fn sample_jsonl() -> String {
    [
        json!({"type": "session", "version": 3, "id": "cf1", "timestamp": "2026-01-02T03:04:05.000Z", "cwd": "/repo"}).to_string(),
        json!({"type": "message", "id": "u1", "parentId": null, "timestamp": "2026-01-02T03:04:06.000Z",
            "message": {"role": "user", "content": [{"type": "text", "text": "read main.rs"}], "timestamp": 1}}).to_string(),
        json!({"type": "message", "id": "a1", "parentId": "u1", "timestamp": "2026-01-02T03:04:07.000Z",
            "message": {"role": "assistant", "content": [
                {"type": "toolCall", "id": "c1", "name": "read", "arguments": {"path": "/repo/main.rs"}}],
                "model": "claude-opus-4-8", "stopReason": "toolUse", "timestamp": 2}}).to_string(),
    ]
    .join("\n")
        + "\n"
}

#[test]
fn campfire_marker_is_distinct() {
    assert_eq!(Campfire::NAME, "campfire");
}

#[test]
fn store_round_trip_and_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let store = CampfireStore::new(dir.path());
    let src = dir.path().join("orig.jsonl");
    std::fs::write(&src, sample_jsonl()).unwrap();

    let loaded = store.load(&src).unwrap();
    assert_eq!(loaded.meta.id, "cf1");

    let saved = store.save(&loaded).unwrap();
    let reloaded = store.load(&saved.reference).unwrap();
    assert_eq!(loaded.body, reloaded.body);

    // The pi `read` tool normalizes the same way (path -> file_path).
    let common = Campfire::to_common(&loaded).unwrap();
    assert_eq!(common.body.len(), 2);
    assert_eq!(common.body[0].role, Role::User);
    assert!(matches!(
        &common.body[1].content[0],
        Block::ToolUse { tool: Tool::Read { file_path, .. }, .. } if file_path == "/repo/main.rs"
    ));
}
