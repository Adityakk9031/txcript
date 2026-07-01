#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Campfire is pi under a different home. These tests confirm the delegation:
//! it reads/writes the identical format and produces the identical conversion,
//! only stamped as a different harness.

use serde_json::json;
use txcript::common;
use txcript::harness::campfire;
use txcript::{Codec, Harness, Store};

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
    assert_eq!(campfire::Campfire::NAME, "campfire");
}

#[test]
fn store_round_trip_and_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let store = campfire::CampfireStore::new(dir.path());
    let src = dir.path().join("orig.jsonl");
    std::fs::write(&src, sample_jsonl()).unwrap();

    let loaded = store.load(&src).unwrap();
    assert_eq!(loaded.meta.id, "cf1");

    let saved = store.save(&loaded).unwrap();
    let reloaded = store.load(&saved.reference).unwrap();
    assert_eq!(loaded.body, reloaded.body);

    // The pi `read` tool normalizes the same way (path -> file_path).
    let converted = campfire::Campfire::to_common(&loaded).unwrap();
    assert_eq!(converted.body.len(), 2);
    assert_eq!(converted.body[0].role, common::Role::User);
    assert!(matches!(
        &converted.body[1].content[0],
        common::Block::ToolUse { tool: common::Tool::Read { file_path, .. }, .. } if file_path == "/repo/main.rs"
    ));
}
