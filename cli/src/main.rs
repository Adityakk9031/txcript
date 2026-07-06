//! A small CLI over the `txcript` crate — the offline half of replay's
//! `continue --local`: discover the AI coding sessions already on this machine
//! and continue one in any harness, writing its native, resumable format.
//!
//! ```text
//! txcript list                          # all local sessions, every harness
//!     [--from <harness>]                    #   only this harness's sessions
//! txcript continue <id>                 # continue <id>, then launch the harness
//!     [--with <harness>]                    #   ...continuing in <harness> instead
//!     [--from <harness>]                    #   scope the id lookup to one harness
//!     [--out <dir>]                         #   write under <dir>; implies --no-resume
//!     [--no-resume]                         #   write the session but don't launch
//! txcript query '<pattern>'             # one-shot search, print ranked hits
//! txcript query                         # fzf-style picker; Enter continues
//!     [--from <harness>]                    #   search only <harness> (default: all)
//!     [--with <harness>]                    #   continue the pick in <harness>
//! ```
//!
//! By default `continue` hands the terminal to the harness (on Unix it `exec`s,
//! replacing this process), launched from the session's own working directory
//! when it still exists — the world the transcript assumes. The resume command
//! is overridable per harness via `TRANSCRIPT_<HARNESS>_RESUME_CMD` (a `{id}`
//! template).
//!
//! All the actual work lives in [`txcript::local`] and [`txcript::search`];
//! this crate is argument parsing (clap) and terminal presentation.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use txcript::{HarnessId, local};

const HARNESSES: &str = "harnesses: claude_code, codex, opencode, pi, campfire, cursor, grok";

#[derive(Parser)]
#[command(
    name = "txcript",
    version,
    about = "List, search, and continue local AI coding sessions in any harness",
    after_help = HARNESSES
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List local sessions across every harness, newest first
    #[command(after_help = HARNESSES)]
    List {
        /// List only this harness's sessions
        #[arg(long, value_name = "HARNESS", value_parser = harness)]
        from: Option<HarnessId>,
    },
    /// Continue a session, then launch its harness
    ///
    /// Same-harness continues resume the original in place; --with
    /// re-synthesizes into another harness's native, resumable format first.
    #[command(after_help = HARNESSES)]
    Continue {
        /// Session id, or its exact title
        id: String,
        /// Continue in this harness instead of the session's own
        #[arg(long, value_name = "HARNESS", value_parser = harness)]
        with: Option<HarnessId>,
        /// Only look for the session in this harness
        #[arg(long, value_name = "HARNESS", value_parser = harness)]
        from: Option<HarnessId>,
        /// Write under this directory instead of the harness's live root
        /// (implies --no-resume: the harness wouldn't see the copy)
        #[arg(long, value_name = "DIR")]
        out: Option<PathBuf>,
        /// Write the session but don't launch the harness
        #[arg(long)]
        no_resume: bool,
    },
    /// Search session content; without a pattern, open an fzf-style picker
    ///
    /// A pattern prints ranked hits, labeled by what matched (user text,
    /// assistant text, thinking, tool use, session metadata). The picker
    /// filters per keystroke; Enter continues the selection, Esc cancels.
    #[command(after_help = HARNESSES)]
    Query {
        /// fzf-style pattern ('exact, ^prefix, suffix$, !not); omit to pick
        /// interactively
        pattern: Option<String>,
        /// Continue the picked session in this harness
        #[arg(long, value_name = "HARNESS", value_parser = harness)]
        with: Option<HarnessId>,
        /// Search only this harness
        #[arg(long, value_name = "HARNESS", value_parser = harness)]
        from: Option<HarnessId>,
    },
}

fn harness(s: &str) -> Result<HarnessId, txcript::Error> {
    s.parse()
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::List { from } => {
            cmd_list(from);
            Ok(ExitCode::SUCCESS)
        }
        Command::Continue {
            id,
            with,
            from,
            out,
            no_resume,
        } => cmd_continue(&id, with, from, out.as_ref(), no_resume),
        Command::Query {
            pattern,
            with,
            from,
        } => query::cmd_query(pattern, with, from),
    };
    result.unwrap_or_else(|e| {
        eprintln!("error: {e}");
        ExitCode::FAILURE
    })
}

fn cmd_list(from: Option<HarnessId>) {
    let sessions = discover_with_spinner();
    let listed: Vec<_> = sessions
        .iter()
        .filter(|s| from.is_none_or(|h| s.harness == h))
        .collect();
    if listed.is_empty() {
        match from {
            Some(h) => println!("no local {h} sessions found"),
            None => println!("no local sessions found"),
        }
    } else {
        let color = style::enabled();
        let header = format!("{:<12}  {:<38}  TITLE / FIRST MESSAGE", "HARNESS", "ID");
        println!("{}", style::dim(&header, color));
        for s in listed {
            let label = s
                .meta
                .title
                .clone()
                .unwrap_or_else(|| s.meta.cwd.clone().unwrap_or_default());
            println!(
                "{}  {}  {}",
                style::harness(s.harness, 12, color),
                style::dim(&format!("{:<38}", truncate(&s.meta.id, 38)), color),
                truncate(&label, 60)
            );
        }
    }
}

/// ANSI styling for the printing commands: colors reach a terminal, plain
/// text reaches a pipe or redirect (and everywhere when `NO_COLOR` is set).
/// Padding happens before coloring — escape bytes would otherwise count
/// against the column width.
mod style {
    use std::io::IsTerminal;

    use txcript::HarnessId;

    pub fn enabled() -> bool {
        std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
    }

    /// [`enabled`], but for output on stderr (the status lines).
    pub fn enabled_err() -> bool {
        std::env::var_os("NO_COLOR").is_none() && std::io::stderr().is_terminal()
    }

    pub fn dim(s: &str, on: bool) -> String {
        if on {
            format!("\x1b[2m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    /// The harness name padded to `pad`, in its color when `on`. Each harness
    /// keeps a stable color so a mixed listing reads at a glance.
    pub fn harness(h: HarnessId, pad: usize, on: bool) -> String {
        let name = format!("{:<pad$}", h.as_str());
        if on {
            format!("{}{name}\x1b[0m", color(h))
        } else {
            name
        }
    }

    const fn color(h: HarnessId) -> &'static str {
        match h {
            HarnessId::ClaudeCode => "\x1b[33m", // yellow
            HarnessId::Codex => "\x1b[36m",      // cyan
            HarnessId::OpenCode => "\x1b[32m",   // green
            HarnessId::Pi => "\x1b[35m",         // magenta
            HarnessId::Campfire => "\x1b[91m",   // bright red
            HarnessId::Cursor => "\x1b[34m",     // blue
            HarnessId::Grok => "\x1b[37m",       // white
        }
    }
}

fn cmd_continue(
    id: &str,
    with: Option<HarnessId>,
    from: Option<HarnessId>,
    out: Option<&PathBuf>,
    no_resume: bool,
) -> Result<ExitCode, String> {
    // Locate the session by exact id or title, optionally scoped to one harness.
    let sessions = discover_with_spinner();
    let found = sessions
        .iter()
        .find(|s| {
            from.is_none_or(|h| s.harness == h)
                && (s.meta.id == id || s.meta.title.as_deref() == Some(id))
        })
        .ok_or_else(|| match from {
            Some(h) => format!("no {h} session matches `{id}` (try `txcript list`)"),
            None => format!("no local session matches `{id}` (try `txcript list`)"),
        })?;

    // Resuming an `--out` copy can't work — the harness reads its live root, not
    // our redirect — so a redirect implies "write only".
    let resume = out.is_none() && !no_resume;
    continue_session(found, with, out.map(PathBuf::as_path), resume)
}

/// Continue `found` in `with` (default: its own harness): same-harness resumes
/// in place, cross-harness re-synthesizes; then exec the harness if `resume`.
fn continue_session(
    found: &local::Session,
    with: Option<HarnessId>,
    out: Option<&std::path::Path>,
    resume: bool,
) -> Result<ExitCode, String> {
    let target = with.unwrap_or(found.harness);

    let resume_id = if target == found.harness && out.is_none() {
        // Fast path: same harness, live root — the session is already on disk.
        // Don't re-synthesize over it (that would round-trip through Common and
        // could shed detail); resume the original in place. The resume line
        // is the whole story, so nothing is announced here.
        found.meta.id.clone()
    } else {
        let common = found.read().map_err(|e| e.to_string())?;
        let written = local::write(target, &common, out).map_err(|e| e.to_string())?;
        let on = style::enabled();
        println!(
            "{} → {}  {}",
            style::harness(found.harness, 0, on),
            style::harness(target, 0, on),
            // `location` is Debug-rendered by the lib (its Ref is generic);
            // shed the quotes it puts around paths.
            style::dim(written.location.trim_matches('"'), on)
        );
        written.id
    };

    let (bin, args) = local::resume_command(target, &resume_id);
    if resume {
        // Hand the terminal to the harness — replaces this process on Unix.
        let workdir = resume_workdir(found.meta.cwd.as_deref());
        let shown = std::iter::once(&bin)
            .chain(&args)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        match &workdir {
            Some(dir) => eprintln!(
                "resuming: {shown} {}",
                style::dim(&format!("(in {})", dir.display()), style::enabled_err())
            ),
            None => eprintln!("resuming: {shown}"),
        }
        // A beat between announcing and exec'ing, and the only clean cancel
        // window (after the exec, ctrl-c hits the harness): a glance at the
        // line is ~300ms and choice reaction another ~300ms, while past ~1s
        // a pause stops reading as deliberate and starts reading as lag —
        // 600ms catches the flinch without breaking flow. Scripts (stderr
        // piped) don't pay it.
        if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
            std::thread::sleep(std::time::Duration::from_millis(600));
        }
        handoff(&bin, &args, workdir.as_deref())
    } else {
        println!("  resume with: {} {}", bin, args.join(" "));
        Ok(ExitCode::SUCCESS)
    }
}

/// The directory to resume in: the session's own cwd — the world its
/// transcript assumes (CLAUDE.md, the git repo, every relative path) —
/// or the current directory, with a warning, when the recorded one no
/// longer exists.
fn resume_workdir(cwd: Option<&str>) -> Option<PathBuf> {
    cwd.filter(|c| !c.is_empty()).and_then(|c| {
        let dir = PathBuf::from(c);
        if dir.is_dir() {
            Some(dir)
        } else {
            eprintln!("warning: session cwd `{c}` no longer exists; resuming from the current directory");
            None
        }
    })
}

fn discover_with_spinner() -> Vec<local::Session> {
    let spinner = spin::Spinner::start("searching local sessions…");
    let sessions = local::discover_with(|harness, count| {
        spinner.set(format!("scanning {harness}… ({count} found)"));
    });
    // No summary: whatever the caller does next (the table, the index count,
    // the resume line) already says what was found.
    spinner.finish();
    sessions
}

/// Replace this process with the harness so it owns the terminal (a true
/// handoff), from `workdir` when given. On non-Unix, spawn-and-wait, then
/// report the child's code.
#[cfg(unix)]
fn handoff(bin: &str, args: &[String], workdir: Option<&std::path::Path>) -> Result<ExitCode, String> {
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args);
    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }
    // `exec` only returns if it failed to launch.
    let e = cmd.exec();
    Err(format!("failed to launch `{bin}`: {e} (is it on PATH?)"))
}

#[cfg(not(unix))]
fn handoff(bin: &str, args: &[String], workdir: Option<&std::path::Path>) -> Result<ExitCode, String> {
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args);
    if let Some(dir) = workdir {
        cmd.current_dir(dir);
    }
    let status = cmd
        .status()
        .map_err(|e| format!("failed to launch `{bin}`: {e} (is it on PATH?)"))?;
    Ok(match status.code() {
        // `ExitCode` is u8-wide; a child code outside 0..=255 still reports
        // failure, just not the exact value.
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        // No code (killed by a signal-equivalent): treated as success, as the
        // previous `exit(code.unwrap_or(0))` did.
        None => ExitCode::SUCCESS,
    })
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

/// A tiny background spinner on stderr, so a slow scan shows it's alive.
/// No-op when stderr isn't a terminal (piped or redirected output stays clean).
mod spin {
    use std::io::{IsTerminal, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    pub struct Spinner {
        running: Arc<AtomicBool>,
        label: Arc<Mutex<String>>,
        handle: Option<JoinHandle<()>>,
        active: bool,
    }

    impl Spinner {
        pub fn start(initial: &str) -> Self {
            let active = std::io::stderr().is_terminal();
            let running = Arc::new(AtomicBool::new(true));
            let label = Arc::new(Mutex::new(initial.to_string()));
            let handle = active.then(|| {
                let (running, label) = (running.clone(), label.clone());
                thread::spawn(move || {
                    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
                    let mut err = std::io::stderr();
                    let mut i = 0;
                    while running.load(Ordering::Relaxed) {
                        let text = label.lock().map(|g| g.clone()).unwrap_or_default();
                        let _ = write!(err, "\r\x1b[2K{} {text}", FRAMES[i % FRAMES.len()]);
                        let _ = err.flush();
                        i += 1;
                        thread::sleep(Duration::from_millis(80));
                    }
                })
            });
            Self {
                running,
                label,
                handle,
                active,
            }
        }

        pub fn set(&self, text: String) {
            if self.active
                && let Ok(mut g) = self.label.lock()
            {
                *g = text;
            }
        }

        /// Stop the animation and clear the line, leaving nothing behind:
        /// the spinner narrates progress, never results.
        pub fn finish(self) {
            self.running.store(false, Ordering::Relaxed);
            if let Some(h) = self.handle {
                let _ = h.join();
            }
            if self.active {
                let mut err = std::io::stderr();
                let _ = write!(err, "\r\x1b[2K");
                let _ = err.flush();
            }
        }
    }
}

// ── query: one-shot search and the fzf-style picker ─────────────────────

mod query {
    use std::collections::HashMap;

    use txcript::search::{DocKey, DocMatch, Index, Origin, Query};
    use txcript::{HarnessId, local};

    type Sessions = HashMap<(HarnessId, String), local::Session>;

    pub(super) fn cmd_query(
        pattern: Option<String>,
        with: Option<HarnessId>,
        from: Option<HarnessId>,
    ) -> Result<std::process::ExitCode, String> {
        let (index, sessions) = build_index(from);
        match pattern {
            Some(pattern) => {
                if with.is_some() {
                    eprintln!("note: --with only affects the interactive picker; ignored here");
                }
                one_shot(&index, &pattern);
                Ok(std::process::ExitCode::SUCCESS)
            }
            None => match tui::pick(&index)? {
                // Cancelled; terminal already restored, nothing to continue.
                None => Ok(std::process::ExitCode::SUCCESS),
                Some(key) => {
                    let session = sessions
                        .get(&(key.harness, key.id.clone()))
                        .ok_or("picked session vanished from the map")?;
                    drop(index);
                    super::continue_session(session, with, None, true)
                }
            },
        }
    }

    /// Load every local session (scoped to `--from` if given) into a hot
    /// index, keyed back to its `Session` for the continue step.
    fn build_index(from: Option<HarnessId>) -> (Index, Sessions) {
        let found = super::discover_with_spinner();
        let spinner = super::spin::Spinner::start("indexing…");
        let mut index = Index::new();
        let mut sessions: Sessions = HashMap::new();
        let total = found.len();
        let scoped = found
            .into_iter()
            .enumerate()
            .filter(|(_, session)| from.is_none_or(|h| session.harness == h));
        for (i, session) in scoped {
            if i % 32 == 0 {
                spinner.set(format!("indexing… ({i}/{total})"));
            }
            // Unreadable sessions are skipped, matching discover.
            if let Ok(common) = session.read() {
                index.insert(
                    DocKey {
                        harness: session.harness,
                        id: session.meta.id.clone(),
                    },
                    &common,
                );
                sessions.insert((session.harness, session.meta.id.clone()), session);
            }
        }
        // No summary: the picker's own counter shows the total, and one-shot
        // output follows immediately.
        spinner.finish();
        (index, sessions)
    }

    /// Print ranked hits for a pattern, colorized when stdout is a terminal.
    fn one_shot(index: &Index, pattern: &str) {
        use std::io::IsTerminal;
        let mut q = Query::fuzzy(pattern);
        q.limit = Some(20);
        q.hits_per_doc = Some(3);
        let matches = index.query(&q);
        if matches.is_empty() {
            println!("no matches for `{pattern}`");
        } else {
            let color = std::io::stdout().is_terminal();
            for m in &matches {
                println!("{}", doc_line(m, color));
                for hit in &m.hits {
                    println!(
                        "  [{:>11}] {}",
                        origin_label(hit.origin),
                        highlight(&hit.line, &hit.spans, 120, color)
                    );
                }
            }
        }
    }

    /// What kind of content a hit came from, spelled out — matches the
    /// [`Origin`] names one to one, wide enough to never abbreviate.
    pub(super) fn origin_label(origin: Origin) -> &'static str {
        match origin {
            Origin::User => "user",
            Origin::Assistant => "assistant",
            Origin::Thinking => "thinking",
            Origin::ToolUse => "tool_use",
            Origin::ToolResult => "tool_result",
            Origin::Meta => "meta",
        }
    }

    fn doc_line(m: &DocMatch<'_>, color: bool) -> String {
        let label = m
            .meta
            .title
            .clone()
            .or_else(|| m.meta.cwd.as_deref().map(basename))
            .unwrap_or_default();
        let date = m.meta.timestamp.format("%Y-%m-%d %H:%M");
        format!(
            "{}  {}  {}  {}",
            crate::style::harness(m.key.harness, 0, color),
            crate::style::dim(&m.key.id, color),
            crate::style::dim(&date.to_string(), color),
            label
        )
    }

    pub(super) fn basename(path: &str) -> String {
        std::path::Path::new(path)
            .file_name()
            .map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned())
    }

    /// Render `line` truncated to `width` chars, match spans emphasized.
    pub(super) fn highlight(
        line: &str,
        spans: &[std::ops::Range<u32>],
        width: usize,
        color: bool,
    ) -> String {
        let mut out = String::new();
        let mut in_span = false;
        for (i, ch) in line.chars().take(width).enumerate() {
            let i = u32::try_from(i).unwrap_or(u32::MAX);
            let now = spans.iter().any(|s| s.contains(&i));
            if color && now != in_span {
                out.push_str(if now { "\x1b[1;31m" } else { "\x1b[0m" });
                in_span = now;
            }
            out.push(ch);
        }
        if color && in_span {
            out.push_str("\x1b[0m");
        }
        if line.chars().count() > width {
            out.push('…');
        }
        out
    }

    // ── the picker ───────────────────────────────────────────────────────

    #[cfg(unix)]
    mod tui {
        use std::io::{IsTerminal, Read, Write};
        use std::process::{Command, Stdio};

        use txcript::search::{DocKey, Index, Query};

        /// Raw-mode + alternate-screen guard: constructing enters, dropping
        /// restores — including on error paths and cancels.
        struct Term {
            saved: String,
        }

        impl Term {
            fn enter() -> Result<Term, String> {
                let saved = stty(&["-g"])?.trim().to_string();
                // min 0 time 1: reads poll at 100ms so a lone ESC is
                // distinguishable from an escape sequence.
                stty(&["raw", "-echo", "min", "0", "time", "1"])?;
                print!("\x1b[?1049h\x1b[?25l");
                let _ = std::io::stdout().flush();
                Ok(Term { saved })
            }
        }

        fn term_size() -> (usize, usize) {
            let parse = |s: &str| -> Option<(usize, usize)> {
                let mut it = s.split_whitespace();
                Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
            };
            stty(&["size"])
                .ok()
                .and_then(|out| parse(&out))
                .unwrap_or((24, 80))
        }

        impl Drop for Term {
            fn drop(&mut self) {
                print!("\x1b[?25h\x1b[?1049l");
                let _ = std::io::stdout().flush();
                let _ = stty(&[&self.saved]);
            }
        }

        fn stty(args: &[&str]) -> Result<String, String> {
            let out = Command::new("stty")
                .args(args)
                .stdin(Stdio::inherit())
                .output()
                .map_err(|e| format!("stty: {e}"))?;
            if out.status.success() {
                Ok(String::from_utf8_lossy(&out.stdout).into_owned())
            } else {
                Err(format!(
                    "stty {}: {}",
                    args.join(" "),
                    String::from_utf8_lossy(&out.stderr).trim()
                ))
            }
        }

        enum Key {
            Char(char),
            Backspace,
            Clear,
            Up,
            Down,
            Enter,
            Cancel,
            None,
        }

        /// Interactive fuzzy picker over the index. Returns the chosen doc,
        /// or `None` on cancel. The terminal is fully restored either way.
        pub(super) fn pick(index: &Index) -> Result<Option<DocKey>, String> {
            if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
                // Raw mode and the alternate screen need real terminal stdio.
                Err("interactive query needs a terminal (pass a pattern instead)".into())
            } else {
                let term = Term::enter()?;
                let mut input = String::new();
                let mut selected = 0usize;
                let mut stdin = std::io::stdin().lock();

                let picked = 'ui: loop {
                    let (rows, cols) = term_size();
                    let visible = rows.saturating_sub(2).max(1);
                    let mut q = Query::fuzzy(&input);
                    q.limit = Some(visible);
                    q.hits_per_doc = Some(1);
                    let matches = index.query(&q);
                    selected = selected.min(matches.len().saturating_sub(1));
                    render(&input, &matches, selected, index.len(), rows, cols);

                    // Poll until a key changes state (break: re-render) or
                    // settles the pick (break 'ui).
                    loop {
                        match read_key(&mut stdin)? {
                            // A poll timeout: nothing pressed, keep waiting.
                            Key::None => {}
                            Key::Char(c) => {
                                input.push(c);
                                selected = 0;
                                break;
                            }
                            Key::Backspace => {
                                input.pop();
                                selected = 0;
                                break;
                            }
                            Key::Clear => {
                                input.clear();
                                selected = 0;
                                break;
                            }
                            Key::Up => {
                                selected = selected.saturating_sub(1);
                                break;
                            }
                            Key::Down => {
                                selected += 1;
                                break;
                            }
                            // Enter with no match under the cursor: keep
                            // waiting.
                            Key::Enter => {
                                if let Some(m) = matches.get(selected) {
                                    break 'ui Some(m.key.clone());
                                }
                            }
                            Key::Cancel => break 'ui None,
                        }
                    }
                };
                drop(term);
                Ok(picked)
            }
        }

        fn render(
            input: &str,
            matches: &[txcript::search::DocMatch<'_>],
            selected: usize,
            total: usize,
            rows: usize,
            cols: usize,
        ) {
            use std::fmt::Write as _;
            // The match count is post-limit: a full page means "at least".
            let count = if matches.len() >= rows.saturating_sub(2) {
                format!("{}+", matches.len())
            } else {
                matches.len().to_string()
            };
            let mut frame = String::from("\x1b[H\x1b[2J");
            let _ = write!(
                frame,
                "\x1b[1m>\x1b[0m {input}\x1b[7m \x1b[0m\r\n\x1b[2m  {count}/{total}\x1b[0m"
            );
            // Lines are *prefixed* with \r\n: a trailing newline on the last
            // row would scroll the prompt off the top of the screen.
            for (i, m) in matches.iter().take(rows.saturating_sub(2)).enumerate() {
                let line = row(m, cols.saturating_sub(2));
                if i == selected {
                    // The row's internal styling ends in resets that would
                    // kill the selection underline mid-line: re-assert it
                    // after each, and pad to the row edge so the underline
                    // runs the full width.
                    let pad = " ".repeat(cols.saturating_sub(2).saturating_sub(visible_width(&line)));
                    let line = line.replace("\x1b[0m", "\x1b[0m\x1b[4m");
                    let _ = write!(frame, "\r\n\x1b[4m▌{line}{pad}\x1b[0m");
                } else {
                    let _ = write!(frame, "\r\n {line}");
                }
            }
            let mut out = std::io::stdout().lock();
            let _ = out.write_all(frame.as_bytes());
            let _ = out.flush();
        }

        /// One list row: harness, date, label, then the best hit line
        /// prefixed by what kind of content it matched in.
        fn row(m: &txcript::search::DocMatch<'_>, cols: usize) -> String {
            let label = m
                .meta
                .title
                .clone()
                .or_else(|| m.meta.cwd.as_deref().map(super::basename))
                .unwrap_or_default();
            let head = format!(
                "{} \x1b[2m{}\x1b[0m {:<24} ",
                crate::style::harness(m.key.harness, 11, true),
                m.meta.timestamp.format("%m-%d %H:%M"),
                truncate_chars(&label, 24),
            );
            // 11 + 1 + 11 + 1 + 24 + 1 visible chars so far.
            let room = cols.saturating_sub(49);
            let preview = m.hits.first().map_or_else(String::new, |hit| {
                format!(
                    "\x1b[2m{:>11}\x1b[0m {}",
                    super::origin_label(hit.origin),
                    // 11 + 1 for the origin column.
                    super::highlight(&hit.line, &hit.spans, room.saturating_sub(12), true)
                )
            });
            format!("{head}{preview}")
        }

        /// Character width of `s` with its ANSI escape sequences stripped —
        /// what the terminal will actually render.
        fn visible_width(s: &str) -> usize {
            let mut in_escape = false;
            s.chars()
                .filter(|&c| match (in_escape, c) {
                    (false, '\x1b') => {
                        in_escape = true;
                        false
                    }
                    (false, _) => true,
                    // `m` closes every sequence this UI emits (SGR only).
                    (true, 'm') => {
                        in_escape = false;
                        false
                    }
                    (true, _) => false,
                })
                .count()
        }

        fn truncate_chars(s: &str, max: usize) -> String {
            if s.chars().count() <= max {
                s.to_string()
            } else {
                let mut t: String = s.chars().take(max.saturating_sub(1)).collect();
                t.push('…');
                t
            }
        }

        /// Read one key, decoding UTF-8 and the arrow escape sequences. With
        /// `min 0 time 1`, a read can legitimately return nothing.
        // The two `Key::None` arms are deliberately separate: a poll timeout
        // and an unmapped byte are different conditions, kept explicit.
        #[allow(clippy::match_same_arms)]
        fn read_key(stdin: &mut impl Read) -> Result<Key, String> {
            let key = match read_byte(stdin)? {
                // A poll timeout: nothing was pressed.
                None => Key::None,
                Some(0x03) => Key::Cancel, // ctrl-c
                Some(0x0a | 0x0d) => Key::Enter,
                Some(0x7f | 0x08) => Key::Backspace,
                Some(0x15) => Key::Clear, // ctrl-u
                Some(0x0e) => Key::Down,  // ctrl-n
                Some(0x10) => Key::Up,    // ctrl-p
                Some(0x1b) => match read_byte(stdin)? {
                    Some(b'[') => match read_byte(stdin)? {
                        Some(b'A') => Key::Up,
                        Some(b'B') => Key::Down,
                        // Any other (or truncated) CSI sequence: not a
                        // picker key.
                        Some(_) | None => Key::None,
                    },
                    None => Key::Cancel, // a lone ESC
                    // Other escape sequences (alt-chords): not picker keys.
                    Some(_) => Key::None,
                },
                Some(b) if (0x20..0x7f).contains(&b) => Key::Char(b as char),
                Some(b) if b >= 0xc2 => utf8_tail(stdin, b)?,
                // Unmapped control bytes and stray UTF-8 continuation bytes.
                Some(_) => Key::None,
            };
            Ok(key)
        }

        /// Finish a UTF-8 multibyte sequence whose lead byte was `lead`.
        fn utf8_tail(stdin: &mut impl Read, lead: u8) -> Result<Key, String> {
            let len = match lead {
                0xc2..=0xdf => 2,
                0xe0..=0xef => 3,
                _ => 4, // 0xf0 and above (the caller guarantees lead >= 0xc2)
            };
            // `None` folds the whole tail to `None`: a poll timeout
            // mid-sequence means a truncated character, not a key.
            let tail: Option<Vec<u8>> = (1..len)
                .map(|_| read_byte(stdin))
                .collect::<Result<_, _>>()?;
            Ok(tail
                .map(|rest| std::iter::once(lead).chain(rest).collect())
                .and_then(|buf| String::from_utf8(buf).ok())
                .and_then(|s| s.chars().next())
                .map_or(Key::None, Key::Char))
        }

        fn read_byte(stdin: &mut impl Read) -> Result<Option<u8>, String> {
            let mut b = [0u8; 1];
            match stdin.read(&mut b) {
                Ok(0) => Ok(None),
                Ok(_) => Ok(Some(b[0])),
                Err(e) => Err(format!("reading stdin: {e}")),
            }
        }
    }

    #[cfg(not(unix))]
    mod tui {
        use txcript::search::{DocKey, Index};

        pub(super) fn pick(_: &Index) -> Result<Option<DocKey>, String> {
            Err("the interactive picker is unix-only; pass a pattern instead".into())
        }
    }
}
