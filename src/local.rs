//! Local sessions across every harness.
//!
//! The seven operations, in terms of this module:
//!
//! | op        | call |
//! |-----------|------|
//! | list      | [`discover`] |
//! | read      | [`Session::read`] |
//! | write     | [`write()`] |
//! | open      | [`resume_command`] (caller execs it) |
//! | translate | [`Session::read`] + [`write()`] |
//! | continue  | [`Session::read`] + [`write()`] + [`resume_command`] |
//! | delete    | [`Session::delete`] |
//!
//! Everything here uses each harness's default on-disk location. For custom
//! roots, use the per-harness [`Store`]s directly.

use std::path::{Path, PathBuf};

use crate::common::Meta;
use crate::harness::{amp, antigravity, campfire, claude_code, codex, cursor, grok, pi};
use crate::{Codec, Common, Error, HarnessId, Result, Store, Transcript};

#[cfg(feature = "opencode")]
use crate::harness::opencode;

/// One session found on this machine.
pub struct Session {
    pub harness: HarnessId,
    pub meta: Meta,
    locator: Locator,
}

enum Locator {
    Path(PathBuf),
    #[cfg(feature = "opencode")]
    Id(String),
}

/// Every local session, newest first. Unreadable stores and sessions are
/// skipped.
#[must_use]
pub fn discover() -> Vec<Session> {
    discover_with(|_, _| {})
}

/// [`discover`], reporting progress: `on_store(harness, sessions_so_far)` is
/// called before each store is scanned.
#[must_use]
pub fn discover_with(mut on_store: impl FnMut(HarnessId, usize)) -> Vec<Session> {
    fn scan<S>(harness: HarnessId, store: Option<S>, out: &mut Vec<Session>)
    where
        S: Store<Ref = PathBuf>,
    {
        // No store (no home directory) or an unreadable one lists nothing.
        let discovered = store.map_or_else(Vec::new, |s| s.discover().unwrap_or_default());
        out.extend(discovered.into_iter().map(|d| Session {
            harness,
            meta: d.meta,
            locator: Locator::Path(d.reference),
        }));
    }

    let mut out: Vec<Session> = Vec::new();
    on_store(HarnessId::ClaudeCode, out.len());
    scan(
        HarnessId::ClaudeCode,
        claude_code::ClaudeStore::default_root(),
        &mut out,
    );
    on_store(HarnessId::Codex, out.len());
    scan(
        HarnessId::Codex,
        codex::CodexStore::default_root(),
        &mut out,
    );
    on_store(HarnessId::Pi, out.len());
    scan(HarnessId::Pi, pi::PiStore::default_root(), &mut out);
    on_store(HarnessId::Campfire, out.len());
    scan(
        HarnessId::Campfire,
        campfire::CampfireStore::default_root(),
        &mut out,
    );
    on_store(HarnessId::Cursor, out.len());
    scan(
        HarnessId::Cursor,
        cursor::CursorStore::default_root(),
        &mut out,
    );
    on_store(HarnessId::Grok, out.len());
    scan(HarnessId::Grok, grok::GrokStore::default_root(), &mut out);
    on_store(HarnessId::Amp, out.len());
    scan(HarnessId::Amp, amp::AmpStore::default_root(), &mut out);
    on_store(HarnessId::Antigravity, out.len());
    scan(
        HarnessId::Antigravity,
        antigravity::AntigravityStore::default_root(),
        &mut out,
    );

    #[cfg(feature = "opencode")]
    {
        on_store(HarnessId::OpenCode, out.len());
        if let Some(store) = opencode::OpenCodeStore::default_db() {
            for d in store.discover().unwrap_or_default() {
                out.push(Session {
                    harness: HarnessId::OpenCode,
                    meta: d.meta,
                    locator: Locator::Id(d.reference),
                });
            }
        }
    }

    out.sort_by_key(|s| std::cmp::Reverse(s.meta.timestamp));
    out
}

impl Session {
    /// Where the session lives, for display: a path, or a database id.
    #[must_use]
    pub fn location(&self) -> String {
        match &self.locator {
            Locator::Path(p) => p.display().to_string(),
            #[cfg(feature = "opencode")]
            Locator::Id(id) => format!("opencode db session {id}"),
        }
    }

    /// Load and convert to the canonical model.
    ///
    /// # Errors
    /// When the session no longer exists, doesn't parse, or its store's
    /// backend is unavailable.
    pub fn read(&self) -> Result<Transcript<Common>> {
        fn go<S>(store: Option<S>, path: &PathBuf) -> Result<Transcript<Common>>
        where
            S: Store<Ref = PathBuf>,
            S::H: Codec,
        {
            <S::H as Codec>::to_common(&required(store)?.load(path)?)
        }
        match (&self.harness, &self.locator) {
            (HarnessId::ClaudeCode, Locator::Path(p)) => {
                go(claude_code::ClaudeStore::default_root(), p)
            }
            (HarnessId::Codex, Locator::Path(p)) => go(codex::CodexStore::default_root(), p),
            (HarnessId::Pi, Locator::Path(p)) => go(pi::PiStore::default_root(), p),
            (HarnessId::Campfire, Locator::Path(p)) => {
                go(campfire::CampfireStore::default_root(), p)
            }
            (HarnessId::Cursor, Locator::Path(p)) => go(cursor::CursorStore::default_root(), p),
            (HarnessId::Grok, Locator::Path(p)) => go(grok::GrokStore::default_root(), p),
            (HarnessId::Amp, Locator::Path(p)) => go(amp::AmpStore::default_root(), p),
            (HarnessId::Antigravity, Locator::Path(p)) => {
                go(antigravity::AntigravityStore::default_root(), p)
            }
            #[cfg(feature = "opencode")]
            (HarnessId::OpenCode, Locator::Id(id)) => opencode::OpenCode::to_common(
                &required(opencode::OpenCodeStore::default_db())?.load(id)?,
            ),
            _ => Err(mismatch(self.harness)),
        }
    }

    /// Remove the session from its harness (see [`Store::delete`] for what
    /// removal means per backend).
    ///
    /// # Errors
    /// When the session no longer exists or the backend rejects the removal.
    pub fn delete(&self) -> Result<()> {
        fn go<S>(store: Option<S>, path: &PathBuf) -> Result<()>
        where
            S: Store<Ref = PathBuf>,
        {
            required(store)?.delete(path)
        }
        match (&self.harness, &self.locator) {
            (HarnessId::ClaudeCode, Locator::Path(p)) => {
                go(claude_code::ClaudeStore::default_root(), p)
            }
            (HarnessId::Codex, Locator::Path(p)) => go(codex::CodexStore::default_root(), p),
            (HarnessId::Pi, Locator::Path(p)) => go(pi::PiStore::default_root(), p),
            (HarnessId::Campfire, Locator::Path(p)) => {
                go(campfire::CampfireStore::default_root(), p)
            }
            (HarnessId::Cursor, Locator::Path(p)) => go(cursor::CursorStore::default_root(), p),
            (HarnessId::Grok, Locator::Path(p)) => go(grok::GrokStore::default_root(), p),
            (HarnessId::Amp, Locator::Path(p)) => go(amp::AmpStore::default_root(), p),
            (HarnessId::Antigravity, Locator::Path(p)) => {
                go(antigravity::AntigravityStore::default_root(), p)
            }
            #[cfg(feature = "opencode")]
            (HarnessId::OpenCode, Locator::Id(id)) => {
                required(opencode::OpenCodeStore::default_db())?.delete(id)
            }
            _ => Err(mismatch(self.harness)),
        }
    }

    /// The command that resumes this session in its own harness.
    #[must_use]
    pub fn resume_command(&self) -> (String, Vec<String>) {
        resume_command(self.harness, &self.meta.id)
    }
}

/// The outcome of [`write()`]: the id the target harness resumes by, and a
/// human-readable location.
pub struct Written {
    pub id: String,
    pub location: String,
}

/// Persist a canonical transcript in `target`'s native, resumable format.
/// `root` overrides the harness's default on-disk root (file-backed stores
/// only; `OpenCode` always goes through `opencode import`).
///
/// # Errors
/// When conversion to the target fails or its store rejects the write.
pub fn write(
    target: HarnessId,
    common: &Transcript<Common>,
    root: Option<&Path>,
) -> Result<Written> {
    fn go<S>(
        store: Option<S>,
        make: impl FnOnce(PathBuf) -> S,
        root: Option<&Path>,
        common: &Transcript<Common>,
        default_dir: impl FnOnce(S) -> PathBuf,
    ) -> Result<Written>
    where
        S: Store,
        S::H: Codec,
        S::Ref: std::fmt::Debug,
    {
        let store = match root {
            Some(dir) => make(dir.to_path_buf()),
            None => make(default_dir(required(store)?)),
        };
        let native = <S::H as Codec>::from_common(common)?;
        let saved = store.save(&native)?;
        Ok(Written {
            id: saved.id,
            location: format!("{:?}", saved.reference),
        })
    }

    match target {
        HarnessId::ClaudeCode => go(
            claude_code::ClaudeStore::default_root(),
            claude_code::ClaudeStore::new,
            root,
            common,
            |s| s.root,
        ),
        HarnessId::Codex => go(
            codex::CodexStore::default_root(),
            codex::CodexStore::new,
            root,
            common,
            |s| s.sessions_dir,
        ),
        HarnessId::Pi => go(
            pi::PiStore::default_root(),
            pi::PiStore::new,
            root,
            common,
            |s| s.sessions_dir,
        ),
        HarnessId::Campfire => go(
            campfire::CampfireStore::default_root(),
            campfire::CampfireStore::new,
            root,
            common,
            |s| s.sessions_dir,
        ),
        HarnessId::Cursor => go(
            cursor::CursorStore::default_root(),
            cursor::CursorStore::new,
            root,
            common,
            |s| s.chats_dir,
        ),
        HarnessId::Grok => go(
            grok::GrokStore::default_root(),
            grok::GrokStore::new,
            root,
            common,
            |s| s.sessions_dir,
        ),
        // Amp is server-authoritative: threads live on ampcode.com and the
        // CLI has no import, so a locally written thread can never be
        // resumed. Sessions convert *from* amp, never into it.
        HarnessId::Amp => Err(Error::Unconvertible {
            harness: "amp",
            detail: "amp has no thread import (threads are server-side); \
                     sessions cannot be continued into amp — convert from amp \
                     instead"
                .to_string(),
        }),
        HarnessId::Antigravity => go(
            antigravity::AntigravityStore::default_root(),
            antigravity::AntigravityStore::new,
            root,
            common,
            |s| s.root,
        ),
        HarnessId::OpenCode => write_opencode(common),
    }
}

#[cfg(feature = "opencode")]
fn write_opencode(common: &Transcript<Common>) -> Result<Written> {
    let store = required(opencode::OpenCodeStore::default_db())?;
    let native = opencode::OpenCode::from_common(common)?;
    let saved = store.save(&native)?;
    Ok(Written {
        id: saved.id,
        location: "imported via `opencode import`".to_string(),
    })
}

#[cfg(not(feature = "opencode"))]
fn write_opencode(_: &Transcript<Common>) -> Result<Written> {
    Err(Error::Unconvertible {
        harness: "opencode",
        detail: "opencode support not compiled in (enable the `opencode` feature)".to_string(),
    })
}

/// The command that resumes session `id` in `harness` — `(binary, args)`,
/// for the caller to exec or spawn. Overridable per harness with
/// `TRANSCRIPT_<HARNESS>_RESUME_CMD`, a template where `{id}` is substituted
/// (e.g. `TRANSCRIPT_CODEX_RESUME_CMD="codex resume {id}"`).
#[must_use]
pub fn resume_command(harness: HarnessId, id: &str) -> (String, Vec<String>) {
    let key = format!(
        "TRANSCRIPT_{}_RESUME_CMD",
        harness.as_str().to_ascii_uppercase()
    );
    let overridden = std::env::var(&key).ok().and_then(|template| {
        let mut parts = template
            .replace("{id}", id)
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>()
            .into_iter();
        // Ignore empty override templates.
        parts.next().map(|bin| (bin, parts.collect()))
    });
    overridden.unwrap_or_else(|| {
        let id = id.to_string();
        match harness {
            HarnessId::ClaudeCode => ("claude".into(), vec!["--resume".into(), id]),
            HarnessId::Codex => ("codex".into(), vec!["resume".into(), id]),
            HarnessId::OpenCode => ("opencode".into(), vec!["--session".into(), id]),
            HarnessId::Pi => ("pi".into(), vec!["--session".into(), id]),
            HarnessId::Campfire => ("campfire".into(), vec!["--session".into(), id]),
            HarnessId::Cursor => ("agent".into(), vec![format!("--resume={id}")]),
            HarnessId::Grok => ("grok".into(), vec!["--resume".into(), id]),
            HarnessId::Amp => ("amp".into(), vec!["threads".into(), "continue".into(), id]),
            HarnessId::Antigravity => ("agy".into(), vec![format!("--conversation={id}")]),
        }
    })
}

fn required<S>(store: Option<S>) -> Result<S> {
    store.ok_or_else(|| {
        Error::Io(std::io::Error::other(
            "no home directory; use the harness Store directly with an explicit root",
        ))
    })
}

fn mismatch(harness: HarnessId) -> Error {
    Error::Malformed {
        harness: "local",
        detail: format!("locator does not belong to {harness}"),
    }
}
