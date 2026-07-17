//! A compact, one-way text projection of the canonical transcript model.
//!
//! [`to_text`] is intended for LLM context, not archival or round trips. It
//! preserves conversational content and compact tool data while discarding
//! replay-only data: message timestamps, usage, stop reasons, reasoning
//! signatures/encrypted payloads, and inline image bytes.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::common::{Block, Role, ToolOutput};
use crate::{Common, Transcript};

/// Render a canonical transcript as compact, LLM-oriented text.
///
/// The format uses short bracketed labels instead of repeating the canonical
/// JSON schema. Tool-call ids are remapped to session-local integers so tool
/// results remain paired without carrying provider-generated identifiers.
#[must_use]
pub fn to_text(transcript: &Transcript<Common>) -> String {
    let mut out = String::new();
    let meta = &transcript.meta;

    out.push_str("[session]\n");
    field(&mut out, "id", &meta.id);
    field(&mut out, "started", &meta.timestamp.to_rfc3339());
    optional_field(&mut out, "title", meta.title.as_deref());
    optional_field(&mut out, "cwd", meta.cwd.as_deref());
    optional_field(&mut out, "branch", meta.git_branch.as_deref());
    optional_field(&mut out, "model", meta.model.as_deref());

    let mut tool_ids = HashMap::<&str, usize>::new();
    let mut next_tool_id = 1;

    for message in &transcript.body {
        for block in &message.content {
            match block {
                Block::Text { text } => {
                    let label = match message.role {
                        Role::User => "user",
                        Role::Assistant => "assistant",
                    };
                    section(&mut out, label, text);
                }
                Block::Thinking { text, .. } => section(&mut out, "thinking", text),
                Block::ToolUse { id, tool } => {
                    let short_id = short_tool_id(&mut tool_ids, &mut next_tool_id, id);
                    let (name, input) = tool.to_canonical();
                    section(
                        &mut out,
                        &format!("tool {short_id} {}", one_line(&name)),
                        &input.to_string(),
                    );
                }
                Block::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    let short_id = short_tool_id(&mut tool_ids, &mut next_tool_id, tool_use_id);
                    let error = if *is_error { " error" } else { "" };
                    let text = match content {
                        ToolOutput::Text(text) => text.as_str(),
                        ToolOutput::Json(value) => {
                            section(
                                &mut out,
                                &format!("result {short_id}{error}"),
                                &value.to_string(),
                            );
                            continue;
                        }
                    };
                    section(&mut out, &format!("result {short_id}{error}"), text);
                }
                Block::Image { source } => section(
                    &mut out,
                    &format!("image {} omitted", one_line(&source.media_type)),
                    "",
                ),
            }
        }
    }

    out
}

fn field(out: &mut String, name: &str, value: &str) {
    let _ = writeln!(out, "{name}={}", one_line(value));
}

fn optional_field(out: &mut String, name: &str, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty()) {
        field(out, name, value);
    }
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn section(out: &mut String, label: &str, text: &str) {
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
    let _ = writeln!(out, "[{label}]");
    out.push_str(text);
    if !text.ends_with('\n') {
        out.push('\n');
    }
}

fn short_tool_id<'a>(
    ids: &mut HashMap<&'a str, usize>,
    next_id: &mut usize,
    provider_id: &'a str,
) -> usize {
    *ids.entry(provider_id).or_insert_with(|| {
        let id = *next_id;
        *next_id += 1;
        id
    })
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, Utc};
    use serde_json::json;

    use super::*;
    use crate::common::{ImageSource, Message, Meta, StopReason, Tool, Usage};

    fn transcript(body: Vec<Message>) -> Transcript<Common> {
        Transcript::new(
            Meta {
                id: "provider-session-id".into(),
                timestamp: DateTime::<Utc>::UNIX_EPOCH,
                cwd: Some("/work/project".into()),
                git_branch: Some("main".into()),
                title: Some("Fix the parser".into()),
                cli_version: Some("9.9.9".into()),
                model: Some("model-name".into()),
            },
            body,
        )
    }

    fn message(role: Role, content: Vec<Block>) -> Message {
        Message {
            role,
            content,
            timestamp: DateTime::<Utc>::UNIX_EPOCH,
            model: None,
            stop_reason: Some(StopReason::EndTurn),
            usage: Some(Usage {
                input_tokens: 100,
                output_tokens: 20,
                cache_read_input_tokens: None,
                cache_creation_input_tokens: None,
            }),
        }
    }

    #[test]
    fn renders_conversation_and_omits_replay_only_data() {
        let rendered = to_text(&transcript(vec![
            message(
                Role::User,
                vec![Block::Text {
                    text: "Please fix it.".into(),
                }],
            ),
            message(
                Role::Assistant,
                vec![
                    Block::Thinking {
                        text: "I should inspect the file.".into(),
                        signature: Some("large-signature".into()),
                        encrypted: Some("large-encrypted-payload".into()),
                    },
                    Block::Text {
                        text: "I found the issue.".into(),
                    },
                    Block::Image {
                        source: ImageSource {
                            source_type: "base64".into(),
                            media_type: "image/png".into(),
                            data: "large-base64-payload".into(),
                        },
                    },
                ],
            ),
        ]));

        assert!(rendered.contains("[user]\nPlease fix it."));
        assert!(rendered.contains("[thinking]\nI should inspect the file."));
        assert!(rendered.contains("[assistant]\nI found the issue."));
        assert!(rendered.contains("[image image/png omitted]"));
        assert!(!rendered.contains("large-signature"));
        assert!(!rendered.contains("large-encrypted-payload"));
        assert!(!rendered.contains("large-base64-payload"));
        assert!(!rendered.contains("input_tokens"));
        assert!(!rendered.contains("cli_version"));
    }

    #[test]
    fn compacts_tool_json_and_shortens_provider_ids() {
        let rendered = to_text(&transcript(vec![
            message(
                Role::Assistant,
                vec![Block::ToolUse {
                    id: "provider-generated-tool-id-with-many-tokens".into(),
                    tool: Tool::Read {
                        file_path: "src/lib.rs".into(),
                        offset: None,
                        limit: Some(20),
                    },
                }],
            ),
            message(
                Role::User,
                vec![Block::ToolResult {
                    tool_use_id: "provider-generated-tool-id-with-many-tokens".into(),
                    content: ToolOutput::Json(json!({"lines": ["one", "two"]})),
                    is_error: false,
                }],
            ),
        ]));

        assert!(rendered.contains("[tool 1 Read]\n{\"file_path\":\"src/lib.rs\",\"limit\":20}"));
        assert!(rendered.contains("[result 1]\n{\"lines\":[\"one\",\"two\"]}"));
        assert!(!rendered.contains("provider-generated-tool-id-with-many-tokens"));
    }

    #[test]
    fn unmatched_results_still_receive_stable_short_ids() {
        let rendered = to_text(&transcript(vec![message(
            Role::User,
            vec![
                Block::ToolResult {
                    tool_use_id: "second".into(),
                    content: ToolOutput::Text("failed".into()),
                    is_error: true,
                },
                Block::ToolResult {
                    tool_use_id: "second".into(),
                    content: ToolOutput::Text("again".into()),
                    is_error: false,
                },
            ],
        )]));

        assert!(rendered.contains("[result 1 error]\nfailed"));
        assert!(rendered.contains("[result 1]\nagain"));
    }
}
