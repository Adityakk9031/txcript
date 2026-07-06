//! Shared JSONL parse/render for line-based harnesses.

use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::Result;

/// Parse JSONL records, skipping blank and invalid lines.
pub(crate) fn parse<R: DeserializeOwned>(text: &str) -> Vec<R> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<R>(line).ok())
        .collect()
}

/// Render records as newline-terminated JSON, one object per line.
pub(crate) fn render<R: Serialize>(records: &[R]) -> Result<String> {
    let mut out = String::new();
    for record in records {
        out.push_str(&serde_json::to_string(record)?);
        out.push('\n');
    }
    Ok(out)
}

/// A session id derived from a file name (its stem) — the fallback a [`Store`]
/// uses when the session text carried no internal id.
///
/// [`Store`]: crate::Store
pub(crate) fn file_id(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}
