#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Integration tests for the Cursor SQLite chat-store harness.

use chrono::{DateTime, Utc};
use serde_json::json;
use txcript::{
    Block, Codec, Common, Cursor, CursorBlob, Message, Meta, Role, TextCodec, Tool, ToolOutput,
    Transcript,
};
#[cfg(feature = "opencode")]
use txcript::{CursorStore, Store};

fn ts(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

fn sample_common() -> Transcript<Common> {
    let meta = Meta {
        id: "sess-1".into(),
        timestamp: ts("2026-01-02T03:04:05.000Z"),
        cwd: Some("/repo".into()),
        git_branch: None,
        title: Some("Cursor demo".into()),
        cli_version: None,
        model: Some("composer-2.5-fast".into()),
    };
    Transcript::new(
        meta,
        vec![
            Message {
                role: Role::User,
                content: vec![Block::Text {
                    text: "read the file".into(),
                }],
                timestamp: ts("2026-01-02T03:04:06.000Z"),
                model: None,
                stop_reason: None,
                usage: None,
            },
            Message {
                role: Role::Assistant,
                content: vec![
                    Block::Thinking {
                        text: String::new(),
                        signature: None,
                        encrypted: Some("opaque-reasoning".into()),
                    },
                    Block::Text {
                        text: "I'll inspect it.".into(),
                    },
                    Block::ToolUse {
                        id: "tool-1".into(),
                        tool: Tool::Read {
                            file_path: "/repo/README.md".into(),
                            offset: None,
                            limit: None,
                        },
                    },
                ],
                timestamp: ts("2026-01-02T03:04:07.000Z"),
                model: Some("composer-2.5-fast".into()),
                stop_reason: None,
                usage: None,
            },
            Message {
                role: Role::User,
                content: vec![Block::ToolResult {
                    tool_use_id: "tool-1".into(),
                    content: ToolOutput::Text("contents".into()),
                    is_error: false,
                }],
                timestamp: ts("2026-01-02T03:04:08.000Z"),
                model: None,
                stop_reason: None,
                usage: None,
            },
            Message {
                role: Role::Assistant,
                content: vec![Block::Text {
                    text: "README loaded.".into(),
                }],
                timestamp: ts("2026-01-02T03:04:09.000Z"),
                model: Some("composer-2.5-fast".into()),
                stop_reason: None,
                usage: None,
            },
        ],
    )
}

#[test]
#[cfg(feature = "opencode")]
fn save_uses_cursor_chat_store_path() {
    let dir = tempfile::tempdir().unwrap();
    let common = sample_common();
    let native = Cursor::from_common(&common).unwrap();
    let saved = CursorStore::new(dir.path()).save(&native).unwrap();

    assert_eq!(saved.id, "sess-1");
    assert_eq!(
        saved.reference,
        dir.path()
            .join("6530f9eb448d96e7552a3c3a29b6cd2b")
            .join("sess-1")
            .join("store.db")
    );
    assert!(saved.reference.is_file());
    assert!(
        saved
            .reference
            .parent()
            .unwrap()
            .join("meta.json")
            .is_file()
    );
    assert!(
        saved
            .reference
            .parent()
            .unwrap()
            .join("prompt_history.json")
            .is_file()
    );
}

#[test]
#[cfg(feature = "opencode")]
fn discover_and_load_extract_cursor_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let common = sample_common();
    let native = Cursor::from_common(&common).unwrap();
    CursorStore::new(dir.path()).save(&native).unwrap();

    let found = CursorStore::new(dir.path()).discover().unwrap();
    assert_eq!(found.len(), 1);
    let meta = &found[0].meta;
    assert_eq!(meta.id, "sess-1");
    assert_eq!(meta.cwd.as_deref(), Some("/repo"));
    assert_eq!(meta.title.as_deref(), Some("Cursor demo"));
    assert_eq!(meta.model.as_deref(), Some("composer-2.5-fast"));

    let loaded = CursorStore::new(dir.path())
        .load(&found[0].reference)
        .unwrap();
    let round = Cursor::to_common(&loaded).unwrap();
    assert_eq!(round.body.len(), 4);
}

#[test]
fn to_common_preserves_cursor_tool_calls_results_and_reasoning() {
    let common = sample_common();
    let native = Cursor::from_common(&common).unwrap();
    let round = Cursor::to_common(&native).unwrap();

    assert_eq!(round.body[0].content, common.body[0].content);
    assert!(matches!(
        &round.body[1].content[0],
        Block::Thinking {
            encrypted: Some(data),
            ..
        } if data == "opaque-reasoning"
    ));
    assert!(matches!(
        &round.body[1].content[2],
        Block::ToolUse {
            id,
            tool: Tool::Read { file_path, .. },
        } if id == "tool-1" && file_path == "/repo/README.md"
    ));
    assert!(matches!(
        &round.body[2].content[0],
        Block::ToolResult {
            tool_use_id,
            content: ToolOutput::Text(text),
            is_error: false,
        } if tool_use_id == "tool-1" && text == "contents"
    ));
}

#[test]
fn text_codec_round_trips_native_export() {
    let native = Cursor::from_common(&sample_common()).unwrap();
    let text = Cursor::to_text(&native).unwrap();
    let round = Cursor::from_text(&text).unwrap();
    assert_eq!(round.body, native.body);
    assert_eq!(
        Cursor::to_common(&round).unwrap().body,
        sample_common().body
    );
}

#[test]
fn from_common_writes_cursor_resume_state_turns() {
    let native = Cursor::from_common(&sample_common()).unwrap();
    let root = latest_root_blob(&native.body);
    let turn_refs = len_fields(&root.data, 8);

    assert_eq!(turn_refs.len(), 1);
    assert!(len_fields(&root.data, 1).is_empty());

    let turn_id = hex_encode_test(&turn_refs[0]);
    let turn_blob = native
        .body
        .blobs
        .iter()
        .find(|blob| blob.id == turn_id)
        .expect("turn structure blob exists");
    let agent_turns = len_fields(&turn_blob.data, 1);
    assert_eq!(agent_turns.len(), 1);

    let user_refs = len_fields(&agent_turns[0], 1);
    let step_refs = len_fields(&agent_turns[0], 2);
    assert_eq!(user_refs.len(), 1);
    assert!(step_refs.len() >= 3);

    let user_id = hex_encode_test(&user_refs[0]);
    assert!(
        native.body.blobs.iter().any(|blob| blob.id == user_id),
        "user message blob exists"
    );
    for step_ref in step_refs {
        let step_id = hex_encode_test(&step_ref);
        assert!(
            native.body.blobs.iter().any(|blob| blob.id == step_id),
            "step blob {step_id} exists"
        );
    }
}

#[test]
#[cfg(feature = "opencode")]
fn store_round_trip_preserves_non_json_blobs() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let mut native = Cursor::from_common(&sample_common()).unwrap();
    native.body.blobs.insert(
        0,
        CursorBlob {
            id: "binary-internal-state".into(),
            data: vec![0, 159, 146, 150, 1, 2, 3],
        },
    );

    let saved = CursorStore::new(src.path()).save(&native).unwrap();
    let loaded = CursorStore::new(src.path()).load(&saved.reference).unwrap();
    assert_eq!(loaded.body.blobs[0].data, vec![0, 159, 146, 150, 1, 2, 3]);

    let copied = CursorStore::new(dst.path()).save(&loaded).unwrap();
    let reloaded = CursorStore::new(dst.path())
        .load(&copied.reference)
        .unwrap();
    assert_eq!(reloaded.body, loaded.body);
}

#[test]
fn parses_existing_cursor_message_shapes() {
    let body = txcript::harness::cursor::CursorDb {
        blobs: vec![
            CursorBlob {
                id: "system".into(),
                data: serde_json::to_vec(&json!({
                    "role": "system",
                    "content": "You are Cursor."
                }))
                .unwrap(),
            },
            CursorBlob {
                id: "context".into(),
                data: serde_json::to_vec(&json!({
                    "role": "user",
                    "content": "<user_info>\nWorkspace Path: /Users/me/repo\n</user_info>"
                }))
                .unwrap(),
            },
            CursorBlob {
                id: "user".into(),
                data: serde_json::to_vec(&json!({
                    "role": "user",
                    "content": [{"type": "text", "text": "<user_query>\nhello!\n</user_query>"}]
                }))
                .unwrap(),
            },
            CursorBlob {
                id: "assistant".into(),
                data: serde_json::to_vec(&json!({
                    "role": "assistant",
                    "content": [{
                        "type": "tool-call",
                        "toolCallId": "tool-1",
                        "toolName": "StrReplace",
                        "args": {
                            "path": "/repo/a.rs",
                            "old_string": "old",
                            "new_string": "new"
                        }
                    }],
                    "providerOptions": {"cursor": {"modelName": "composer-2.5"}}
                }))
                .unwrap(),
            },
        ],
        meta: Vec::new(),
        session_meta: Some(json!({
            "schemaVersion": 1,
            "createdAtMs": 1767337445000i64,
            "hasConversation": true,
            "title": "Hello Agent",
            "updatedAtMs": 1767337445000i64
        })),
    };
    let transcript = Transcript::<Cursor>::new(
        Meta {
            id: "sess".into(),
            timestamp: ts("2026-01-02T03:04:05.000Z"),
            cwd: Some("/Users/me/repo".into()),
            git_branch: None,
            title: None,
            cli_version: None,
            model: None,
        },
        body,
    );
    let common = Cursor::to_common(&transcript).unwrap();
    assert_eq!(common.body.len(), 2);
    assert!(matches!(
        &common.body[1].content[0],
        Block::ToolUse {
            tool: Tool::Edit { file_path, old_string, new_string, .. },
            ..
        } if file_path == "/repo/a.rs" && old_string == "old" && new_string == "new"
    ));
}

fn latest_root_blob(body: &txcript::harness::cursor::CursorDb) -> &CursorBlob {
    let meta = cursor_meta_json(body);
    let id = meta
        .get("latestRootBlobId")
        .and_then(serde_json::Value::as_str)
        .expect("latest root id");
    body.blobs
        .iter()
        .find(|blob| blob.id == id)
        .expect("latest root blob")
}

fn cursor_meta_json(body: &txcript::harness::cursor::CursorDb) -> serde_json::Value {
    let raw = body
        .meta
        .iter()
        .find(|entry| entry.key == "0")
        .expect("cursor meta row")
        .value
        .as_str();
    let decoded = hex_decode_test(raw);
    serde_json::from_slice(&decoded).expect("cursor meta json")
}

fn len_fields(data: &[u8], wanted_field: u64) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        let Some(key) = read_varint(data, &mut i) else {
            break;
        };
        let field = key >> 3;
        match key & 0x07 {
            0 => {
                let _ = read_varint(data, &mut i);
            }
            1 => i = i.saturating_add(8),
            2 => {
                let Some(len) = read_varint(data, &mut i).map(|v| v as usize) else {
                    break;
                };
                if i + len > data.len() {
                    break;
                }
                let value = data[i..i + len].to_vec();
                if field == wanted_field {
                    out.push(value);
                }
                i += len;
            }
            5 => i = i.saturating_add(4),
            _ => break,
        }
    }
    out
}

fn read_varint(data: &[u8], i: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    let mut shift = 0u32;
    while *i < data.len() && shift < 64 {
        let byte = data[*i];
        *i += 1;
        value |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
        shift += 7;
    }
    None
}

fn hex_decode_test(s: &str) -> Vec<u8> {
    assert_eq!(s.len() % 2, 0);
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn hex_encode_test(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
}
