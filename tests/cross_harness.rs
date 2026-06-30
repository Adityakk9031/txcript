#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! The whole point of the crate: a transcript converts between harnesses
//! through the Common hub without losing the conversation. This chains one
//! transcript Claude -> Codex -> OpenCode -> pi -> Campfire and checks that the
//! semantic content (roles, text, the typed Edit tool and its result) is
//! identical at every hop.
//!
//! Message *grouping* and harness-specific attribution (model, stop_reason,
//! usage, timestamps) legitimately differ across harnesses; the block-level
//! conversation does not. The signature below captures exactly that invariant.

use chrono::{DateTime, Utc};
use txcript::{
    Block, Campfire, ClaudeCode, Codec, Codex, Common, Cursor, Message, Meta, OpenCode, Pi, Role,
    Tool, ToolOutput, Transcript, convert,
};

fn ts(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

/// A flat, grouping-independent fingerprint of the conversation: one line per
/// block, in order, role-tagged. Stable across harnesses that split or merge
/// messages differently.
fn signature(t: &Transcript<Common>) -> Vec<String> {
    let mut out = Vec::new();
    for msg in &t.body {
        let role = match msg.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        for block in &msg.content {
            let desc = match block {
                Block::Text { text } => format!("text:{text}"),
                Block::Thinking { text, .. } => format!("thinking:{text}"),
                Block::ToolUse { id, tool } => match tool {
                    Tool::Edit {
                        file_path,
                        old_string,
                        new_string,
                        ..
                    } => {
                        format!("use:{id}:Edit:{file_path}:{old_string}->{new_string}")
                    }
                    Tool::Bash { command, .. } => format!("use:{id}:Bash:{command}"),
                    Tool::Raw { tool_name, .. } => format!("use:{id}:Raw:{tool_name}"),
                    other => format!("use:{id}:{other:?}"),
                },
                Block::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    let text = match content {
                        ToolOutput::Text(s) => s.clone(),
                        ToolOutput::Json(v) => v.to_string(),
                    };
                    format!("result:{tool_use_id}:{is_error}:{text}")
                }
                Block::Image { source } => format!("image:{}", source.media_type),
            };
            out.push(format!("{role}/{desc}"));
        }
    }
    out
}

/// Single-block-per-turn so every harness's grouping agrees: a user ask, an
/// assistant Edit, the tool result, and a closing line.
fn sample() -> Transcript<Common> {
    let meta = Meta {
        id: "x1".into(),
        timestamp: ts("2026-01-02T03:04:05.000Z"),
        cwd: Some("/repo".into()),
        git_branch: None,
        title: Some("Cross".into()),
        cli_version: None,
        model: Some("claude-opus-4-8".into()),
    };
    let model = || Some("claude-opus-4-8".to_string());
    let body = vec![
        Message {
            role: Role::User,
            content: vec![Block::Text {
                text: "fix the bug".into(),
            }],
            timestamp: ts("2026-01-02T03:04:06.000Z"),
            model: None,
            stop_reason: None,
            usage: None,
        },
        Message {
            role: Role::Assistant,
            content: vec![Block::ToolUse {
                id: "call-1".into(),
                tool: Tool::Edit {
                    file_path: "/repo/a.rs".into(),
                    old_string: "i <= n".into(),
                    new_string: "i < n".into(),
                    replace_all: false,
                },
            }],
            timestamp: ts("2026-01-02T03:04:07.000Z"),
            model: model(),
            stop_reason: None,
            usage: None,
        },
        Message {
            role: Role::User,
            content: vec![Block::ToolResult {
                tool_use_id: "call-1".into(),
                content: ToolOutput::Text("patched".into()),
                is_error: false,
            }],
            timestamp: ts("2026-01-02T03:04:07.000Z"),
            model: None,
            stop_reason: None,
            usage: None,
        },
        Message {
            role: Role::Assistant,
            content: vec![Block::Text {
                text: "done".into(),
            }],
            timestamp: ts("2026-01-02T03:04:08.000Z"),
            model: model(),
            stop_reason: None,
            usage: None,
        },
    ];
    Transcript::new(meta, body)
}

#[test]
fn conversation_survives_every_hop() {
    let common = sample();
    let expected = signature(&common);

    // Land it in Claude, then walk it across every harness via the hub.
    let claude = ClaudeCode::from_common(&common).unwrap();
    assert_eq!(
        signature(&ClaudeCode::to_common(&claude).unwrap()),
        expected,
        "claude"
    );

    let codex = convert::<ClaudeCode, Codex>(&claude).unwrap();
    assert_eq!(
        signature(&Codex::to_common(&codex).unwrap()),
        expected,
        "codex"
    );

    let opencode = convert::<Codex, OpenCode>(&codex).unwrap();
    assert_eq!(
        signature(&OpenCode::to_common(&opencode).unwrap()),
        expected,
        "opencode"
    );

    let pi = convert::<OpenCode, Pi>(&opencode).unwrap();
    assert_eq!(signature(&Pi::to_common(&pi).unwrap()), expected, "pi");

    let campfire = convert::<Pi, Campfire>(&pi).unwrap();
    assert_eq!(
        signature(&Campfire::to_common(&campfire).unwrap()),
        expected,
        "campfire"
    );

    let cursor = convert::<Campfire, Cursor>(&campfire).unwrap();
    assert_eq!(
        signature(&Cursor::to_common(&cursor).unwrap()),
        expected,
        "cursor"
    );

    // And all the way back to Claude.
    let round = convert::<Cursor, ClaudeCode>(&cursor).unwrap();
    assert_eq!(
        signature(&ClaudeCode::to_common(&round).unwrap()),
        expected,
        "claude (round)"
    );
}
