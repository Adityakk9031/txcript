//! `txcript view` — print a session as compact text.
//!
//! The source is a session id or exact title, looked up like `continue`,
//! with an optional `#range` fragment (see `fragment.rs`). Output goes to
//! stdout, colorless and pager-free, so it pipes cleanly into `pbcopy` or an
//! LLM prompt. Message numbers are printed in the output (`── #N ──` rules),
//! so what you see is what you reference.

use std::process::ExitCode;

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

pub fn cmd_view(source: &str, from: Option<HarnessId>) -> Result<ExitCode, String> {
    let (common, request) = load_source(source, from)?;

    let total = common.body.len();
    let span = match &request {
        Some(req) => req.resolve(total)?,
        None => Span(0..total),
    };
    // `resolve` bounds-checked against `total`, so the render always lands.
    let rendered = text::to_text_fragment(&common, &span)
        .ok_or_else(|| format!("range is out of bounds — the session has {total} messages"))?;
    // A failed write means the reader is gone (`txcript view … | head`):
    // finish quietly instead of panicking the way `print!` would.
    let _ = std::io::Write::write_all(&mut std::io::stdout(), rendered.as_bytes());
    Ok(ExitCode::SUCCESS)
}
