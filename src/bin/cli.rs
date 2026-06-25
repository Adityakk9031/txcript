//! A small CLI over the `transcript` crate — the offline half of replay's
//! `continue --local`: discover the AI coding sessions already on this machine
//! and convert one from its harness into another's native, resumable format.
//!
//! ```text
//! transcript list                          # all local sessions, every harness
//! transcript convert <id> --to <harness>   # convert <id> into <harness>'s format
//!     [--out <dir>]                         #   write under <dir> instead of the live root
//! ```
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
        Some("convert") => cmd_convert(&args[1..]),
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
        "transcript — convert local AI coding sessions between harnesses\n\n\
         usage:\n  \
         transcript list\n  \
         transcript convert <id> --to <harness> [--out <dir>]\n\n\
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

fn cmd_convert(args: &[String]) -> Result<(), String> {
    let mut id: Option<String> = None;
    let mut target: Option<HarnessId> = None;
    let mut out: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--to" => {
                i += 1;
                let v = args.get(i).ok_or("--to needs a harness")?;
                target = Some(v.parse().map_err(|e: transcript::Error| e.to_string())?);
            }
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).ok_or("--out needs a directory")?));
            }
            other if id.is_none() => id = Some(other.to_string()),
            other => return Err(format!("unexpected argument `{other}`")),
        }
        i += 1;
    }

    let id = id.ok_or("missing session id (try `transcript list`)")?;
    let target = target.ok_or("missing --to <harness>")?;

    // Locate the session by exact id or title across every harness.
    let sessions = discover_all();
    let found = sessions
        .iter()
        .find(|s| s.meta.id == id || s.meta.title.as_deref() == Some(id.as_str()))
        .ok_or_else(|| format!("no local session matches `{id}` (try `transcript list`)"))?;

    let common = load_common(found)?;
    if found.harness == target {
        eprintln!("note: source and target are both {target}; converting through Common anyway");
    } else {
        eprintln!("converting {} -> {target}", found.harness);
    }

    let (new_id, location) = save_target(target, &common, out.as_deref())?;
    println!("wrote {target} session {new_id}");
    println!("  at {location}");
    Ok(())
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
