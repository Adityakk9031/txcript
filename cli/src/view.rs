//! `txcript view` — inspect a session in the terminal or print compact text.
//!
//! The source is a session id or exact title, looked up like `continue`,
//! with an optional `#range` fragment (see `fragment.rs`). A terminal gets a
//! readable, colored view through a pager; `--no-pager` writes that view
//! directly. A pipe or redirect keeps the established compact, colorless text
//! projection. Both presentations print message numbers, so what you see is
//! what you reference.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::IsTerminal as _;
use std::process::ExitCode;
use std::process::{Command, Stdio};

use txcript::common::{ArtifactSource, Block, Message, Role, ToolOutput};
use txcript::{Common, HarnessId, Span, Transcript, text};

use crate::fragment;

/// Resolve a `view`/`export` source — a session id or exact title, with
/// an optional `#range` — to the session's canonical transcript and the
/// parsed range request, if any.
pub fn load_source(
    source: &str,
    from: Option<HarnessId>,
) -> Result<(Transcript<Common>, Option<fragment::SpanReq>), String> {
    let sessions = super::discover_with_spinner(from)?;
    // A whole-input match (a title that itself contains `#12`) beats the
    // fragment interpretation.
    let (src, request) = match fragment::parse_ref(source) {
        (_, Some(_)) if super::find_exact(&sessions, from, source).is_some() => (source, None),
        parsed => parsed,
    };

    let session = super::find_session(&sessions, from, src)?.ok_or_else(|| {
        let (origin, scope) = if from == Some(HarnessId::ClaudeChat) {
            ("Claude Chat", String::new())
        } else {
            ("local", from.map_or(String::new(), |h| format!(" {h}")))
        };
        format!(
            "no {origin}{scope} session matches `{src}` (try `{} list`)",
            crate::program()
        )
    })?;
    let common = session
        .read()
        .map_err(|e| format!("reading session `{src}`: {e}"))?;
    Ok((common, request))
}

pub fn cmd_view(source: &str, from: Option<HarnessId>, no_pager: bool) -> Result<ExitCode, String> {
    let (common, request) = load_source(source, from)?;

    let total = common.body.len();
    let span = match &request {
        Some(req) => req.resolve(total)?,
        None => Span(0..total),
    };
    let stdout_is_terminal = std::io::stdout().is_terminal();
    let width = terminal_size::terminal_size()
        .map_or(80, |(terminal_size::Width(width), _)| usize::from(width))
        .clamp(40, 120);
    let presentation = presentation_for(
        stdout_is_terminal,
        std::env::var_os("NO_COLOR").is_some(),
        width,
    );
    // `resolve` bounds-checked against `total`, so the render always lands.
    let rendered = render_output(&common, &span, presentation)
        .ok_or_else(|| format!("range is out of bounds — the session has {total} messages"))?;
    let txcript_pager = nonempty_env("TXCRIPT_PAGER");
    let pager = nonempty_env("PAGER");
    output(
        rendered.as_bytes(),
        pager_for(
            stdout_is_terminal,
            no_pager,
            txcript_pager.as_deref(),
            pager.as_deref(),
        ),
    )?;
    Ok(ExitCode::SUCCESS)
}

#[derive(Clone, Copy)]
enum Presentation {
    Compact,
    Human { color: bool, width: usize },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pager<'a> {
    Direct,
    Command(&'a str),
    Less,
}

const fn pager_for<'a>(
    stdout_is_terminal: bool,
    no_pager: bool,
    txcript_pager: Option<&'a str>,
    pager: Option<&'a str>,
) -> Pager<'a> {
    if !stdout_is_terminal || no_pager {
        Pager::Direct
    } else if let Some(command) = txcript_pager {
        Pager::Command(command)
    } else if let Some(command) = pager {
        Pager::Command(command)
    } else {
        Pager::Less
    }
}

const fn presentation_for(stdout_is_terminal: bool, no_color: bool, width: usize) -> Presentation {
    if stdout_is_terminal {
        Presentation::Human {
            color: !no_color,
            width,
        }
    } else {
        Presentation::Compact
    }
}

fn render_output(
    common: &Transcript<Common>,
    span: &Span,
    presentation: Presentation,
) -> Option<String> {
    match presentation {
        Presentation::Compact => text::to_text_fragment(common, span),
        Presentation::Human { color, width } => render_human(common, span, color, width),
    }
}

fn render_human(
    common: &Transcript<Common>,
    span: &Span,
    color: bool,
    width: usize,
) -> Option<String> {
    let messages = common.fragment(span)?;
    let mut out = String::new();
    human_header(&mut out, common, span, color);
    human_messages(&mut out, messages, span.0.start, color, width);
    Some(out)
}

fn human_header(out: &mut String, common: &Transcript<Common>, span: &Span, color: bool) {
    let title = common.meta.title.as_deref().unwrap_or(&common.meta.id);
    let _ = writeln!(out, "{}", paint("1;36", &terminal_label_safe(title), color));
    human_field(out, "ID", &common.meta.id, color);
    human_field(out, "Started", &common.meta.timestamp.to_rfc3339(), color);
    if let Some(cwd) = common.meta.cwd.as_deref().filter(|value| !value.is_empty()) {
        human_field(out, "Directory", cwd, color);
    }
    if let Some(branch) = common
        .meta
        .git_branch
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        human_field(out, "Branch", branch, color);
    }
    if let Some(model) = common
        .meta
        .model
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        human_field(out, "Model", model, color);
    }
    let shown = match span.0.len() {
        0 => "none".to_string(),
        1 => format!("#{}", span.0.start + 1),
        _ => format!("#{}–{}", span.0.start + 1, span.0.end),
    };
    human_field(
        out,
        "Messages",
        &format!("{shown} of {}", common.body.len()),
        color,
    );
}

fn human_messages(out: &mut String, messages: &[Message], start: usize, color: bool, width: usize) {
    let mut tool_ids = HashMap::<&str, usize>::new();
    let mut next_tool_id = 1usize;
    for (offset, message) in messages.iter().enumerate() {
        let ordinal = start + offset + 1;
        let role = match message.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
        };
        human_rule(out, &format!("Message #{ordinal} · {role}"), width, color);
        for block in &message.content {
            match block {
                Block::Text { text } => human_body(out, text),
                Block::Thinking { text, .. } => {
                    human_section(out, "Thinking", "2;35", color);
                    human_body(out, text);
                }
                Block::ToolUse { id, tool } => {
                    let short_id = human_tool_id(&mut tool_ids, &mut next_tool_id, id);
                    let (name, input) = tool.to_canonical();
                    human_section(
                        out,
                        &format!("Tool #{short_id} · {}", terminal_label_safe(&name)),
                        "1;33",
                        color,
                    );
                    let body = match input {
                        serde_json::Value::Null => String::new(),
                        serde_json::Value::Object(ref map) if map.is_empty() => String::new(),
                        value => serde_json::to_string_pretty(&value)
                            .unwrap_or_else(|_| value.to_string()),
                    };
                    human_body(out, &body);
                }
                Block::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    let short_id = human_tool_id(&mut tool_ids, &mut next_tool_id, tool_use_id);
                    let suffix = if *is_error { " · Error" } else { "" };
                    human_section(
                        out,
                        &format!("Result #{short_id}{suffix}"),
                        if *is_error { "1;31" } else { "1;32" },
                        color,
                    );
                    match content {
                        ToolOutput::Text(text) => human_body(out, text),
                        ToolOutput::Json(value) => {
                            let body = serde_json::to_string_pretty(value)
                                .unwrap_or_else(|_| value.to_string());
                            human_body(out, &body);
                        }
                    }
                }
                Block::Image { source } => human_section(
                    out,
                    &format!(
                        "Image · {} omitted",
                        terminal_label_safe(&source.media_type)
                    ),
                    "2;34",
                    color,
                ),
                Block::Artifact { artifact } => {
                    human_section(
                        out,
                        &format!("Artifact · {}", terminal_label_safe(&artifact.name)),
                        "1;34",
                        color,
                    );
                    let body = match &artifact.source {
                        ArtifactSource::Path { path, .. } => path.as_str(),
                        ArtifactSource::Text { text, .. } => text.as_str(),
                        ArtifactSource::Base64 { .. } => "binary data omitted",
                    };
                    human_body(out, body);
                }
            }
        }
    }
}

fn human_field(out: &mut String, label: &str, value: &str, color: bool) {
    let _ = writeln!(
        out,
        "{}  {}",
        paint("2", &format!("{label:<10}"), color),
        terminal_label_safe(value)
    );
}

fn human_rule(out: &mut String, label: &str, width: usize, color: bool) {
    let prefix = format!("── {label} ");
    let suffix = "─".repeat(width.saturating_sub(prefix.chars().count()).max(2));
    let _ = writeln!(
        out,
        "\n{}",
        paint("1;36", &format!("{prefix}{suffix}"), color)
    );
}

fn human_section(out: &mut String, label: &str, code: &str, color: bool) {
    let _ = writeln!(out, "\n{}", paint(code, &format!("▸ {label}"), color));
}

fn human_body(out: &mut String, text: &str) {
    out.push_str(&terminal_safe(text));
    if !text.ends_with('\n') {
        out.push('\n');
    }
}

fn human_tool_id<'a>(
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

fn terminal_safe(text: &str) -> String {
    text.chars()
        .flat_map(|ch| match ch {
            '\n' | '\t' => ch.to_string().chars().collect::<Vec<_>>(),
            ch if ch.is_control() => ch.escape_default().collect(),
            ch => vec![ch],
        })
        .collect()
}

fn terminal_label_safe(text: &str) -> String {
    text.chars()
        .flat_map(|ch| {
            if ch.is_control() {
                ch.escape_default().collect()
            } else {
                vec![ch]
            }
        })
        .collect()
}

fn paint(code: &str, text: &str, color: bool) -> String {
    if color {
        format!("\x1b[{code}m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn output(bytes: &[u8], pager: Pager<'_>) -> Result<(), String> {
    let (mut command, configured) = match pager {
        Pager::Direct => {
            return write_stream(std::io::stdout().lock(), bytes)
                .map_err(|error| format!("writing stdout: {error}"));
        }
        Pager::Command(command) => (shell_command(command), true),
        Pager::Less => {
            let mut command = Command::new("less");
            command.arg("-FRX");
            (command, false)
        }
    };
    command.stdin(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if !configured && error.kind() == std::io::ErrorKind::NotFound => {
            return write_stream(std::io::stdout().lock(), bytes)
                .map_err(|error| format!("writing stdout: {error}"));
        }
        Err(error) => return Err(format!("starting pager: {error}")),
    };
    if let Some(stdin) = child.stdin.take()
        && let Err(error) = write_stream(stdin, bytes)
    {
        // Do not leave a pager behind if its stdin fails for a reason other
        // than the normal early-close/BrokenPipe case handled by
        // `write_stream`.
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("writing to pager: {error}"));
    }
    let status = child
        .wait()
        .map_err(|error| format!("waiting for pager: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("pager exited with {status}"))
    }
}

#[cfg(unix)]
fn shell_command(command: &str) -> Command {
    let mut shell = Command::new("sh");
    shell.arg("-c").arg(command);
    shell
}

#[cfg(windows)]
fn shell_command(command: &str) -> Command {
    let mut shell = Command::new("cmd");
    shell.arg("/C").arg(command);
    shell
}

fn write_stream(mut writer: impl std::io::Write, bytes: &[u8]) -> Result<(), std::io::Error> {
    match writer.write_all(bytes) {
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use chrono::DateTime;
    use clap::Parser as _;
    use txcript::common::{Block, Message, Meta, Role};

    use super::*;

    fn transcript() -> Transcript<Common> {
        Transcript::new(
            Meta {
                id: "session-123".into(),
                timestamp: DateTime::UNIX_EPOCH,
                cwd: Some("/work/project".into()),
                git_branch: Some("main".into()),
                title: Some("Fix the parser".into()),
                cli_version: None,
                model: Some("test-model".into()),
            },
            vec![Message {
                role: Role::User,
                content: vec![Block::Text {
                    text: "Please fix it.".into(),
                }],
                timestamp: DateTime::UNIX_EPOCH,
                model: None,
                stop_reason: None,
                usage: None,
            }],
        )
    }

    #[test]
    fn terminal_rendering_is_human_facing_while_pipeline_rendering_stays_compact() {
        let common = transcript();
        let span = Span(0..1);

        let terminal = render_output(
            &common,
            &span,
            Presentation::Human {
                color: false,
                width: 60,
            },
        )
        .unwrap();
        let pipeline = render_output(&common, &span, Presentation::Compact).unwrap();

        assert!(terminal.contains("Fix the parser"));
        assert!(terminal.contains("Message #1 · User"));
        assert!(terminal.contains("Please fix it."));
        assert!(!terminal.contains("[session]"));

        assert!(pipeline.starts_with("[session]\n"));
        assert!(pipeline.contains("── #1 ──\n[user]\nPlease fix it."));
    }

    #[test]
    fn stdout_terminal_selects_the_human_presentation() {
        assert!(matches!(
            presentation_for(true, false, 72),
            Presentation::Human {
                color: true,
                width: 72
            }
        ));
        assert!(matches!(
            presentation_for(true, true, 72),
            Presentation::Human { color: false, .. }
        ));
        assert!(matches!(
            presentation_for(false, false, 72),
            Presentation::Compact
        ));
    }

    #[test]
    fn human_ranges_keep_full_session_ordinals() {
        let mut common = transcript();
        let template = common.body[0].clone();
        common.body = vec![template; 7];

        let rendered = render_output(
            &common,
            &Span(4..7),
            Presentation::Human {
                color: false,
                width: 60,
            },
        )
        .unwrap();

        assert!(rendered.contains("Messages    #5–7 of 7"));
        assert!(rendered.contains("Message #5 · User"));
        assert!(rendered.contains("Message #6 · User"));
        assert!(rendered.contains("Message #7 · User"));
        assert!(!rendered.contains("Message #4 · User"));
    }

    #[test]
    fn no_pager_is_accepted_by_the_view_command() {
        let cli =
            crate::Cli::try_parse_from(["txcript", "view", "session-123", "--no-pager"]).unwrap();
        assert!(matches!(
            cli.command,
            crate::Command::Session(crate::SessionCommand::View { no_pager: true, .. })
        ));
    }

    #[test]
    fn pager_selection_respects_terminal_flags_and_environment() {
        assert_eq!(
            pager_for(true, false, Some("custom --flag"), Some("fallback")),
            Pager::Command("custom --flag")
        );
        assert_eq!(
            pager_for(true, false, None, Some("fallback")),
            Pager::Command("fallback")
        );
        assert_eq!(pager_for(true, false, None, None), Pager::Less);
        assert_eq!(pager_for(true, true, None, None), Pager::Direct);
        assert_eq!(
            pager_for(false, false, Some("custom"), Some("fallback")),
            Pager::Direct
        );
    }

    struct FailingWriter(io::ErrorKind);

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(self.0))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn only_broken_pipe_output_errors_are_ignored() {
        assert!(write_stream(FailingWriter(io::ErrorKind::BrokenPipe), b"text").is_ok());
        let error = write_stream(FailingWriter(io::ErrorKind::WriteZero), b"text").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WriteZero);
    }

    #[test]
    fn human_output_escapes_terminal_control_sequences() {
        let safe = terminal_safe("before\x1b]52;c;payload\x07after\r\nnext\tcolumn");
        assert!(
            !safe
                .chars()
                .any(|ch| ch.is_control() && ch != '\n' && ch != '\t')
        );
        assert!(safe.contains("\\u{1b}]52;c;payload\\u{7}after\\r"));
        assert!(safe.ends_with("\nnext\tcolumn"));
    }

    #[test]
    fn human_labels_escape_line_breaks_and_tabs() {
        let safe = terminal_label_safe("title\nforged field\tvalue\r");
        assert_eq!(safe, "title\\nforged field\\tvalue\\r");
        assert!(!safe.chars().any(char::is_control));
    }

    #[cfg(unix)]
    #[test]
    fn configured_pager_receives_the_rendered_text() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("pager-output");
        let command = format!("cat > {}", destination.display());

        output(b"rendered session\n", Pager::Command(&command)).unwrap();

        assert_eq!(
            std::fs::read_to_string(destination).unwrap(),
            "rendered session\n"
        );
    }
}
