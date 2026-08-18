<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="docs/assets/wordmark-dark.svg">
    <img src="docs/assets/wordmark-light.svg" alt="txcript" width="600">
  </picture>
</p>

<p align="center">Convert coding-agent session transcripts between harness formats — and continue any session in any harness.</p>

<p align="center">
  English | <a href="README.ja.md">日本語</a> | <a href="README.zh-CN.md">简体中文</a> | <a href="README.zh-TW.md">繁體中文</a> | <a href="README.ko.md">한국어</a> | <a href="README.de.md">Deutsch</a> | <a href="README.es.md">Español</a> | <a href="README.fr.md">Français</a> | <a href="README.it.md">Italiano</a> | <a href="README.pt-BR.md">Português (Brasil)</a> | <a href="README.ru.md">Русский</a>
</p>

<p align="center">
  <a href="https://crates.io/crates/txcript"><img src="https://img.shields.io/crates/v/txcript?logo=rust&color=4c71f2" alt="crates.io"></a>
  <a href="https://www.npmjs.com/package/txcript"><img src="https://img.shields.io/npm/v/txcript?logo=npm&color=4c71f2" alt="npm"></a>
  <a href="https://docs.rs/txcript"><img src="https://img.shields.io/docsrs/txcript?logo=docsdotrs" alt="docs.rs"></a>
  <a href="https://github.com/skillsynchq/txcript/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/skillsynchq/txcript/ci.yml?branch=main&logo=github&label=ci" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-555" alt="License"></a>
</p>

<p align="center">
  <a href="https://claude.com/claude-code"><img src="https://github.com/anthropics.png?size=160" alt="Claude Code" height="44" width="44"></a>
  &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://github.com/openai/codex"><img src="https://github.com/openai.png?size=160" alt="Codex" height="44" width="44"></a>
  &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://opencode.ai"><img src="https://opencode.ai/apple-touch-icon-v3.png" alt="OpenCode" height="44" width="44"></a>
  &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://pi.dev"><img src="https://pi.dev/logo-auto.svg" alt="pi" height="44" width="44"></a>
  &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://cursor.com"><img src="https://github.com/cursor.png?size=160" alt="Cursor" height="44" width="44"></a>
</p>

Start a session in Claude Code, hit a usage limit or a wall, and pick it up in
Codex — with the full conversation, reasoning, and tool history intact:

```console
$ txcript list
  claude_code   2h ago   fix relay reconnect bug          9f3a21…
  codex         1d ago   wire up usage accounting         c41b8d…
  opencode      3d ago   migrate store to sqlite          77e0f2…

$ txcript continue 9f3a21 --with codex    # re-synthesize into Codex, then launch it
```

txcript maps each harness's native transcript format through a typed common
model. Native load/save is byte-lossless; cross-harness conversion preserves
messages, reasoning, tool calls, tool results, images, metadata, and usage
where available. It ships as a **Rust library**, a **CLI**, and a prebuilt
**WASM module** for Bun, Node, and browsers.

## Highlights

- **9 harnesses, one model** — every format converts through
  `Transcript<Common>`, so adding a harness connects it to all the others.
- **Byte-lossless round-trips** — loading and saving a session in its own
  format reproduces it exactly.
- **Continue anywhere** — `txcript continue <id> --with <harness>` rewrites a
  session into another harness's native format and launches it. The original
  is never modified.
- **Search everything** — fuzzy/substring search across every session on the
  machine (fzf-style syntax, powered by [nucleo](https://github.com/helix-editor/nucleo)),
  as a library API, a one-shot CLI query, or an interactive picker.
- **MCP server** — `txcript mcp` exposes read-only `list_sessions`,
  `search_sessions`, and `read_session` tools, so agents can mine past
  sessions as context.
- **Documented formats** — every harness's on-disk format is written up in
  [`docs/formats/`](docs/formats), with provenance for each claim (official
  docs, source permalinks, or reverse-engineering notes).

## Supported harnesses

| Harness | id | Format doc | Notes |
|---|---|---|---|
| [Claude Code](https://claude.com/claude-code) | `claude_code` | [claude-code.md](docs/formats/claude-code.md) | |
| [Codex](https://github.com/openai/codex) | `codex` | [codex.md](docs/formats/codex.md) | |
| [OpenCode](https://opencode.ai) | `opencode` | [opencode.md](docs/formats/opencode.md) | |
| [pi](https://pi.dev) | `pi` | [pi.md](docs/formats/pi.md) | |
| Campfire | `campfire` | [campfire.md](docs/formats/campfire.md) | |
| [Cursor](https://cursor.com) | `cursor` | [cursor.md](docs/formats/cursor.md) | |
| [Grok CLI](https://github.com/xai-org/grok-build) | `grok` | [grok.md](docs/formats/grok.md) | |
| [Amp](https://ampcode.com) | `amp` | [amp.md](docs/formats/amp.md) | Convert *from* only — threads are server-side and the CLI has no import |
| [Antigravity](https://antigravity.google) | `antigravity` | [antigravity.md](docs/formats/antigravity.md) | |

The string ids are what the CLI and WASM APIs take.

## Install

**CLI** (installs the `txcript` binary):

```sh
cargo install --git https://github.com/skillsynchq/txcript txcript-cli
# or from a checkout: cargo install --path cli
```

**Rust library**:

```sh
cargo add txcript
```

**JS / TS** (prebuilt WASM, no Rust toolchain needed):

```sh
bun add txcript     # or: npm install txcript
```

## CLI

Discover local sessions and continue one in any harness:

```sh
txcript list                             # local sessions across every harness
txcript continue <id>[#range]            # continue <id>, then launch its harness
    [--with <harness>]                    #   ...continuing in <harness> instead
    [--from <harness>]                    #   scope the id lookup to one harness
    [--out <dir>]                         #   write under <dir>; implies --no-resume
    [--no-resume]                         #   write the session but don't launch
txcript view <id>[#range]                # print a session as compact text
    [--from <harness>]                    #   scope the id lookup to one harness
```

`continue` hands the terminal to the harness when done (on Unix it `exec`s).
Same-harness continues resume the original in place; `--with` re-synthesizes
into another harness's native format first. A cross-harness continue leaves
the original session where it was — what is written is always a copy; the
source is never modified or removed. Override the launch command per harness
with `TRANSCRIPT_<HARNESS>_RESUME_CMD` (a `{id}` template), e.g.
`TRANSCRIPT_CODEX_RESUME_CMD="codex resume {id}"`.

`view` prints a token-conscious text projection with a `── #N ──` rule
numbering each message. `#range` names a 1-based, inclusive message range —
`abc#7` is message 7, `abc#5-12`, `abc#5-` (from 5 on), `abc#-10` (through
10) — and the printed ordinals are the ones ranges use, so what you see is
what you reference. `continue` accepts the same suffix and continues just
those messages as a new session; ranges that cut a tool call away from its
result are refused, with the nearest valid range suggested.

### Search

```sh
txcript query 'relay bug'                # one-shot: ranked hits, highlighted
txcript query                            # fzf-style picker; Enter continues
    [--from <harness>]                   #   search only <harness> (default: all)
    [--with <harness>]                   #   continue the pick in <harness>
```

The picker is dependency-free (raw-mode ANSI): type to filter with fzf-style
fuzzy syntax, arrows / ctrl-p/n to move, Enter to continue the selection in
its own harness (or `--with`), Esc to cancel. Every row shows which kind of
content matched — user text, assistant text, thinking, tool use, tool output,
or session metadata.

### MCP server

```sh
txcript mcp                              # stdio transport
```

Exposes exactly three read-only tools; their optional filters match the CLI:

- `list_sessions(from?, cwd?)`
- `search_sessions(pattern, from?, cwd?)`
- `read_session(id, from?)`

Omitting `from` includes every harness. Omitting `cwd` applies no directory
filter, including sessions without a recorded working directory; when `cwd` is
present, those sessions do not match.

### Shell completions

```sh
txcript completion zsh > ~/.zfunc/_txcript      # or wherever your fpath looks
source <(txcript completion bash)               # bash, ad hoc
txcript completion fish > ~/.config/fish/completions/txcript.fish
```

## Rust library

```toml
[dependencies]
txcript = "0.5"
# Drops the OpenCode SQLite store (rusqlite); the OpenCode codec stays available.
# txcript = { version = "0.5", default-features = false }
```

Three layers, smallest to largest:

- `Codec` — `to_common` / `from_common` per harness; `convert::<A, B>` chains
  them through the canonical model.
- `TextCodec` — `from_text` / `to_text`: parse/render a harness's native session
  text, no I/O.
- `Store` — discover/load/save against a real backend (session directories, or
  SQLite DBs for OpenCode and Cursor).

Convert in memory (no filesystem):

```rust
use txcript::harness::{claude_code, codex};
use txcript::{Codec, TextCodec, convert};

let claude = claude_code::ClaudeCode::from_text(jsonl_text)?;          // Transcript<ClaudeCode>
let codex = convert::<claude_code::ClaudeCode, codex::Codex>(&claude)?; // Transcript<Codex>
let codex_text = codex::Codex::to_text(&codex)?;                       // native rollout JSONL
```

Or go through disk with a `Store`:

```rust
use txcript::harness::{claude_code, codex};
use txcript::{Store, convert};

let store = claude_code::ClaudeStore::default_root().expect("home dir");
let found = store.discover()?;                       // cheap metadata scan
let claude = store.load(&found[0].reference)?;       // Transcript<ClaudeCode>

let codex = convert::<_, codex::Codex>(&claude)?;
codex::CodexStore::default_root().expect("home dir").save(&codex)?;  // resumable on disk
```

The canonical model is `Transcript<Common>` — `Meta` + `Vec<Message>`, where a
`Message` holds typed `Block`s (`Text`, `Thinking`, `ToolUse`, `ToolResult`,
`Image`) and a typed `Tool` enum.

A slash command the user ran at the harness is a `Tool::Command` on a user
turn, with whatever the harness printed back as the paired `ToolResult` — so
`/release patch` reads as a call rather than as the markup the harness happens
to record it in. The leading `/` is what marks it canonically: no model-facing
tool name has one. Boilerplate the harness regenerates on its own (Claude
Code's local-command caveat) does not survive into the model.

### Search (feature `search`, on by default)

`txcript::search` supports fuzzy and substring search over transcripts via
[nucleo](https://github.com/helix-editor/nucleo). One-shot search:

```rust
use txcript::search::{Query, search};

let hits = search(&common, &Query::fuzzy("relay bug"));   // fzf syntax: 'exact ^prefix !not
for hit in hits {
    // hit.origin: User | Assistant | Thinking | ToolUse | ToolResult | Meta
    // hit.span addresses the message; hit.highlights are char ranges into hit.line
    let messages = common.fragment(&hit.span);            // zero-copy: Option<&[Message]>
}
```

For picker-style search, build an `Index` once and query it per keystroke:

```rust
use txcript::search::{DocKey, Index, Query};

let mut index = Index::new();
index.insert(DocKey { harness, id }, &common);   // re-insert replaces; caller owns refresh
let matches = index.query(&Query::fuzzy("srch")); // ranked docs, best lines as hits
```

An empty pattern returns documents newest-first. Tool outputs are excluded by
default; use `Origin::ALL` to include them. `Query.harnesses`, `Query.limit`,
and `Query.hits_per_doc` narrow results.

### Text projection

`txcript::text::to_text(&common)` is a one-way, token-conscious projection of
`Transcript<Common>` for use as LLM context. It keeps messages, reasoning
text, and compact tool calls/results while omitting replay-only payloads such
as encrypted reasoning, usage accounting, and inline image bytes.
`to_text_fragment(&common, &span)` renders a `Span` of the body in the same
format with `── #N ──` rules carrying each message's 1-based ordinal in the
full session — the numbering `txcript view` prints.

## WASM module (Bun / Node / browsers)

The pure codec compiles to WebAssembly; the JS host owns all I/O and calls in
for the transformation. The `Store` layer (filesystem, SQLite, subprocess)
stays native and is excluded from the WASM build. The npm package ships the
wasm prebuilt:

```sh
bun add txcript     # or: npm install txcript
```

```ts
import { convert, toCommon, fromCommon, harnesses } from "txcript";
import { readFileSync, writeFileSync } from "node:fs";

const input = readFileSync("rollout.jsonl", "utf8");

// native -> native (e.g. a Codex rollout into Claude Code's JSONL)
writeFileSync("session.jsonl", convert(input, "codex", "claude_code"));

// canonical view, and back
const common = JSON.parse(toCommon(input, "codex"));   // { meta, messages }
const pi = fromCommon(JSON.stringify(common), "pi");

harnesses(); // ["claude_code","codex","opencode","pi","campfire","cursor","grok","amp","antigravity"]
```

Text-in / text-out: `input` is a harness's native session text (JSONL for
claude_code/codex/pi/campfire, the `opencode export` JSON for opencode, a
JSON export of Cursor's `store.db` for cursor, a JSON bundle of the session
directory's files for grok, the thread JSON document — the
`amp threads export` shape — for amp, and a JSON dump of the conversation
database — hex-encoded protobuf step blobs — for antigravity); the result is
the target's native text. Invalid harness names or unparseable input throw a
JS `Error`.

To build the wasm from source instead:

```sh
git clone https://github.com/skillsynchq/txcript.git
cd txcript
bun run setup        # once: wasm target + wasm-bindgen-cli
bun run build        # produces ./pkg
```

## Format documentation

Not all of these transcript formats are documented by their vendors.
[`docs/formats/`](docs/formats) has one document per harness — where sessions
live on disk, how discovery finds them, a dissection of every part of the
format, and its quirks — each tagged with the provenance of what it claims:
official documentation, the harness's own open-source serialization code
(cited with commit-pinned permalinks), or reverse engineering.

## Development

```sh
cargo test                                          # native suite
cargo test --no-default-features                    # without the SQLite store
bun run build && bun examples/convert.ts <file> <from> <to>
```

The binary lives in its own workspace crate (`cli/`, package `txcript-cli`) so
its dependencies (clap) never touch library consumers.

## License

[Apache-2.0](LICENSE)
