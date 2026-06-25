//! A small CLI over the `transcript` crate — the offline half of replay's
//! `continue --local`: discover the AI coding sessions already on this machine
//! and continue one in any harness, writing its native, resumable format.
//!
//! ```text
//! transcript list                          # all local sessions, every harness
//! transcript continue <id>                 # continue <id>, then launch the harness
//!     [--with <harness>]                    #   ...continuing in <harness> instead
//!     [--from <harness>]                    #   scope the id lookup to one harness
//!     [--out <dir>]                         #   write under <dir>; implies --no-resume
//!     [--no-resume]                         #   write the session but don't launch
//! ```
//!
//! By default `continue` hands the terminal to the harness (on Unix it `exec`s,
//! replacing this process). The resume command is overridable per harness via
//! `TRANSCRIPT_<HARNESS>_RESUME_CMD` (a `{id}` template).
//!
//! `<harness>` is one of: claude_code, codex, opencode, pi, campfire.

use std::path::PathBuf;

use transcript::{
    Campfire, CampfireStore, ClaudeCode, ClaudeStore, Codec, Codex, CodexStore, Common, HarnessId,
    Meta, Pi, PiStore, Store, Transcript,
};

/// How to load a discovered session back: a file path, or an OpenCode session id.
#[derive(Clone)]
enum Locator {
    Path(PathBuf),
    Id(String),
}

/// One discovered local session.
struct Found {
    harness: HarnessId,
    meta: Meta,
    locator: Locator,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("list") => cmd_list(),
        Some("continue") => cmd_continue(&args[1..]),
        Some("-h") | Some("--help") | Some("help") | None => {
            usage();
            Ok(())
        }
        Some(other) => {
            usage();
            Err(format!("unknown command `{other}`"))
        }
    }
}

fn usage() {
    eprintln!(
        "transcript — continue local AI coding sessions in any harness\n\n\
         usage:\n  \
         transcript list\n  \
         transcript continue <id> [--with <harness>] [--from <harness>] [--out <dir>] [--no-resume]\n\n\
         continue launches the harness afterward; --with crosses into another,\n\
         --out/--no-resume write the session without launching.\n\
         harnesses: claude_code, codex, opencode, pi, campfire"
    );
}

fn cmd_list() -> Result<(), String> {
    let sessions = discover_all();
    if sessions.is_empty() {
        println!("no local sessions found");
        return Ok(());
    }
    println!("{:<12}  {:<38}  TITLE / FIRST MESSAGE", "HARNESS", "ID");
    for s in &sessions {
        let label = s
            .meta
            .title
            .clone()
            .unwrap_or_else(|| s.meta.cwd.clone().unwrap_or_default());
        println!(
            "{:<12}  {:<38}  {}",
            s.harness.as_str(),
            truncate(&s.meta.id, 38),
            truncate(&label, 60)
        );
    }
    Ok(())
}

fn cmd_continue(args: &[String]) -> Result<(), String> {
    let parse_harness = |v: Option<&String>, flag: &str| -> Result<HarnessId, String> {
        v.ok_or_else(|| format!("{flag} needs a harness"))?
            .parse()
            .map_err(|e: transcript::Error| e.to_string())
    };

    let mut id: Option<String> = None;
    let mut with: Option<HarnessId> = None;
    let mut from: Option<HarnessId> = None;
    let mut out: Option<PathBuf> = None;
    let mut no_resume = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--with" => {
                i += 1;
                with = Some(parse_harness(args.get(i), "--with")?);
            }
            "--from" => {
                i += 1;
                from = Some(parse_harness(args.get(i), "--from")?);
            }
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).ok_or("--out needs a directory")?));
            }
            "--no-resume" => no_resume = true,
            other if id.is_none() => id = Some(other.to_string()),
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }

    let id = id.ok_or("missing session id (try `transcript list`)")?;

    // Locate the session by exact id or title, optionally scoped to one harness.
    let sessions = discover_all();
    let found = sessions
        .iter()
        .find(|s| {
            from.is_none_or(|h| s.harness == h)
                && (s.meta.id == id || s.meta.title.as_deref() == Some(id.as_str()))
        })
        .ok_or_else(|| match from {
            Some(h) => format!("no {h} session matches `{id}` (try `transcript list`)"),
            None => format!("no local session matches `{id}` (try `transcript list`)"),
        })?;

    // Default to continuing in the source's own harness.
    let target = with.unwrap_or(found.harness);

    // Resuming an `--out` copy can't work — the harness reads its live root, not
    // our redirect — so a redirect implies "write only".
    let resume = out.is_none() && !no_resume;

    let resume_id = if target == found.harness && out.is_none() {
        // Fast path: same harness, live root — the session is already on disk.
        // Don't re-synthesize over it (that would round-trip through Common and
        // could shed detail); resume the original in place.
        eprintln!("continuing existing {target} session {}", found.meta.id);
        found.meta.id.clone()
    } else {
        let common = load_common(found)?;
        eprintln!("continuing {} session as {target}", found.harness);
        let (new_id, location) = save_target(target, &common, out.as_deref())?;
        println!("wrote {target} session {new_id}");
        println!("  at {location}");
        new_id
    };

    let (bin, args) = resume_command(target, &resume_id);
    if resume {
        // Hand the terminal to the harness — replaces this process on Unix.
        eprintln!("resuming: {} {}", bin, args.join(" "));
        handoff(&bin, &args)
    } else {
        println!("  resume with: {} {}", bin, args.join(" "));
        Ok(())
    }
}

/// The command that resumes a session in its harness, overridable per harness
/// via `TRANSCRIPT_<HARNESS>_RESUME_CMD` (a template; `{id}` is substituted).
fn resume_command(harness: HarnessId, id: &str) -> (String, Vec<String>) {
    let key = format!("TRANSCRIPT_{}_RESUME_CMD", harness.as_str().to_ascii_uppercase());
    if let Ok(template) = std::env::var(&key) {
        let mut parts = template.replace("{id}", id).split_whitespace().map(String::from).collect::<Vec<_>>().into_iter();
        if let Some(bin) = parts.next() {
            return (bin, parts.collect());
        }
    }
    let id = id.to_string();
    match harness {
        HarnessId::ClaudeCode => ("claude".into(), vec!["--resume".into(), id]),
        HarnessId::Codex => ("codex".into(), vec!["resume".into(), id]),
        HarnessId::OpenCode => ("opencode".into(), vec!["--session".into(), id]),
        HarnessId::Pi => ("pi".into(), vec!["--session".into(), id]),
        HarnessId::Campfire => ("campfire".into(), vec!["--session".into(), id]),
    }
}

/// Replace this process with the harness so it owns the terminal (a true
/// handoff). On non-Unix, spawn-and-wait, then exit with the child's code.
#[cfg(unix)]
fn handoff(bin: &str, args: &[String]) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    // `exec` only returns if it failed to launch.
    let e = std::process::Command::new(bin).args(args).exec();
    Err(format!("failed to launch `{bin}`: {e} (is it on PATH?)"))
}

#[cfg(not(unix))]
fn handoff(bin: &str, args: &[String]) -> Result<(), String> {
    let status = std::process::Command::new(bin)
        .args(args)
        .status()
        .map_err(|e| format!("failed to launch `{bin}`: {e} (is it on PATH?)"))?;
    std::process::exit(status.code().unwrap_or(0));
}

// ── discover / load / save across harnesses ────────────────────────────

fn discover_all() -> Vec<Found> {
    let mut out = Vec::new();

    if let Some(store) = ClaudeStore::default_root() {
        for d in store.discover().unwrap_or_default() {
            out.push(Found {
                harness: HarnessId::ClaudeCode,
                meta: d.meta,
                locator: Locator::Path(d.reference),
            });
        }
    }
    if let Some(store) = CodexStore::default_root() {
        for d in store.discover().unwrap_or_default() {
            out.push(Found {
                harness: HarnessId::Codex,
                meta: d.meta,
                locator: Locator::Path(d.reference),
            });
        }
    }
    if let Some(store) = PiStore::default_root() {
        for d in store.discover().unwrap_or_default() {
            out.push(Found {
                harness: HarnessId::Pi,
                meta: d.meta,
                locator: Locator::Path(d.reference),
            });
        }
    }
    if let Some(store) = CampfireStore::default_root() {
        for d in store.discover().unwrap_or_default() {
            out.push(Found {
                harness: HarnessId::Campfire,
                meta: d.meta,
                locator: Locator::Path(d.reference),
            });
        }
    }
    #[cfg(feature = "opencode")]
    if let Some(store) = transcript::OpenCodeStore::default_db() {
        for d in store.discover().unwrap_or_default() {
            out.push(Found {
                harness: HarnessId::OpenCode,
                meta: d.meta,
                locator: Locator::Id(d.reference),
            });
        }
    }

    out.sort_by_key(|f| std::cmp::Reverse(f.meta.timestamp));
    out
}

fn load_common(found: &Found) -> Result<Transcript<Common>, String> {
    let err = |e: transcript::Error| e.to_string();
    match (&found.harness, &found.locator) {
        (HarnessId::ClaudeCode, Locator::Path(p)) => {
            let store = ClaudeStore::default_root().ok_or("no home directory")?;
            ClaudeCode::to_common(&store.load(p).map_err(err)?).map_err(err)
        }
        (HarnessId::Codex, Locator::Path(p)) => {
            let store = CodexStore::default_root().ok_or("no home directory")?;
            Codex::to_common(&store.load(p).map_err(err)?).map_err(err)
        }
        (HarnessId::Pi, Locator::Path(p)) => {
            let store = PiStore::default_root().ok_or("no home directory")?;
            Pi::to_common(&store.load(p).map_err(err)?).map_err(err)
        }
        (HarnessId::Campfire, Locator::Path(p)) => {
            let store = CampfireStore::default_root().ok_or("no home directory")?;
            Campfire::to_common(&store.load(p).map_err(err)?).map_err(err)
        }
        #[cfg(feature = "opencode")]
        (HarnessId::OpenCode, Locator::Id(id)) => {
            use transcript::OpenCode;
            let store = transcript::OpenCodeStore::default_db().ok_or("no home directory")?;
            OpenCode::to_common(&store.load(id).map_err(err)?).map_err(err)
        }
        _ => Err("unsupported source harness/locator combination".into()),
    }
}

/// Returns `(new_session_id, on-disk location)`.
fn save_target(
    target: HarnessId,
    common: &Transcript<Common>,
    out: Option<&std::path::Path>,
) -> Result<(String, String), String> {
    let err = |e: transcript::Error| e.to_string();
    match target {
        HarnessId::ClaudeCode => {
            let root = file_store_root(out, ClaudeStore::default_root().map(|s| s.root))?;
            let native = ClaudeCode::from_common(common).map_err(err)?;
            describe(ClaudeStore::new(root).save(&native).map_err(err)?)
        }
        HarnessId::Codex => {
            let root = file_store_root(out, CodexStore::default_root().map(|s| s.sessions_dir))?;
            let native = Codex::from_common(common).map_err(err)?;
            describe(CodexStore::new(root).save(&native).map_err(err)?)
        }
        HarnessId::Pi => {
            let root = file_store_root(out, PiStore::default_root().map(|s| s.sessions_dir))?;
            let native = Pi::from_common(common).map_err(err)?;
            describe(PiStore::new(root).save(&native).map_err(err)?)
        }
        HarnessId::Campfire => {
            let root = file_store_root(out, CampfireStore::default_root().map(|s| s.sessions_dir))?;
            let native = Campfire::from_common(common).map_err(err)?;
            describe(CampfireStore::new(root).save(&native).map_err(err)?)
        }
        HarnessId::OpenCode => save_opencode(common),
    }
}

#[cfg(feature = "opencode")]
fn save_opencode(common: &Transcript<Common>) -> Result<(String, String), String> {
    use transcript::{OpenCode, OpenCodeStore};
    let err = |e: transcript::Error| e.to_string();
    let store = OpenCodeStore::default_db().ok_or("no home directory")?;
    let native = OpenCode::from_common(common).map_err(err)?;
    let saved = store.save(&native).map_err(err)?;
    Ok((saved.id, "imported via `opencode import`".into()))
}

#[cfg(not(feature = "opencode"))]
fn save_opencode(_: &Transcript<Common>) -> Result<(String, String), String> {
    Err("opencode support not compiled in (enable the `opencode` feature)".into())
}

fn file_store_root(
    out: Option<&std::path::Path>,
    default: Option<PathBuf>,
) -> Result<PathBuf, String> {
    match out {
        Some(p) => Ok(p.to_path_buf()),
        None => default.ok_or_else(|| "no home directory; pass --out <dir>".to_string()),
    }
}

fn describe<R: std::fmt::Debug>(saved: transcript::Saved<R>) -> Result<(String, String), String> {
    Ok((saved.id, format!("{:?}", saved.reference)))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}
