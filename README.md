# txcript

**txcript lets you swap the agent you're using in the middle of a session.**

It converts session transcripts between Claude Code, Codex, OpenCode, pi,
Campfire, Cursor, and Grok CLI. Your messages, the agent's reasoning, tool calls, and
images all come across, so the new agent picks up the conversation where the
old one left off.

<p align="center">
  <a href="https://claude.com/claude-code"><img src="https://github.com/anthropics.png?size=160" alt="Claude Code" height="48" width="48"></a>
  &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://github.com/openai/codex"><img src="https://github.com/openai.png?size=160" alt="Codex" height="48" width="48"></a>
  &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://opencode.ai"><img src="https://opencode.ai/apple-touch-icon-v3.png" alt="OpenCode" height="48" width="48"></a>
  &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://pi.dev"><img src="https://pi.dev/logo-auto.svg" alt="pi" height="48" width="48"></a>
  &nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;&nbsp;
  <a href="https://cursor.com"><img src="https://github.com/cursor.png?size=160" alt="Cursor" height="48" width="48"></a>
</p>

Each harness has its own native transcript shape. This crate maps those shapes
through a typed common model, then re-emits them for another harness. Native
load/save stays byte-lossless; cross-harness transformation preserves the
semantic conversation: messages, reasoning, tool calls, tool results, images,
metadata, and usage where available.

Supported harnesses (string ids in parentheses, used by the CLI and WASM):

- Claude Code (`claude_code`)
- Codex (`codex`)
- OpenCode (`opencode`)
- pi (`pi`)
- Campfire (`campfire`)
- Cursor (`cursor`)
- Grok CLI (`grok`)

It ships three ways: a **Rust library**, a **CLI**, and a **WASM module** for
Bun / Node / the browser.

## Use as a library

```toml
[dependencies]
txcript = "0.1"
# Drops the OpenCode SQLite store (rusqlite); the OpenCode codec stays available.
# txcript = { version = "0.1", default-features = false }
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

### Search (feature `search`, on by default)

`txcript::search` does fuzzy and substring search over transcripts, built on
[nucleo](https://github.com/helix-editor/nucleo) (helix's matcher). One-shot,
for find-in-transcript:

```rust
use txcript::search::{Query, search};

let hits = search(&common, &Query::fuzzy("relay bug"));   // fzf syntax: 'exact ^prefix !not
for hit in hits {
    // hit.origin: User | Assistant | Thinking | ToolUse | ToolResult | Meta
    // hit.message / hit.block locate it; hit.spans are char ranges for highlighting
}
```

Hot, for a picker that queries per keystroke — insert once, query cheaply and
repeatedly (`Index` is `Send + Sync`, scoring shards across cores natively):

```rust
use txcript::search::{DocKey, Index, Query};

let mut index = Index::new();
index.insert(DocKey { harness, id }, &common);   // re-insert replaces; caller owns refresh
let matches = index.query(&Query::fuzzy("srch")); // ranked docs, best lines as hits
```

An empty pattern lists all documents newest-first (the pre-keystroke picker
state). Tool outputs are excluded from the default search scope (`Origin::ALL`
opts in); `Query.harnesses` scopes to specific harnesses; `Query.limit` caps
returned documents and `Query.hits_per_doc` (default 8) caps materialized
lines per document. Indexing ~600 real sessions (~90 MB of text) takes ~1.5 s;
queries run 3–40 ms.

## Use as a CLI

The binary lives in its own workspace crate (`cli/`, package `txcript-cli`)
so its dependencies (clap) never touch library consumers:

```sh
cargo install --git https://github.com/skillsynchq/txcript txcript-cli   # installs `txcript`
# or from a checkout: cargo install --path cli
```

It discovers local sessions and continues one in any harness — the offline half
of replay's `continue --local`:

```sh
txcript list                             # local sessions across every harness
txcript continue <id>                    # continue <id>, then launch its harness
    [--with <harness>]                    #   ...continuing in <harness> instead
    [--from <harness>]                    #   scope the id lookup to one harness
    [--out <dir>]                         #   write under <dir>; implies --no-resume
    [--no-resume]                         #   write the session but don't launch
```

`continue` hands the terminal to the harness when done (on Unix it `exec`s).
Same-harness continues resume the original in place; `--with` re-synthesizes
into another harness's native format first. Override the launch command per
harness with `TRANSCRIPT_<HARNESS>_RESUME_CMD` (a `{id}` template), e.g.
`TRANSCRIPT_CODEX_RESUME_CMD="codex resume {id}"`.

Search across every session on the machine:

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

## Use as a WASM module (Bun / Node)

The pure codec compiles to WebAssembly; the JS host owns all I/O and calls in
for the transformation. The `Store` layer (filesystem, SQLite, subprocess) stays
native and is excluded from the WASM build.

### Install from git

```sh
bun add git+ssh://git@github.com/skillsynchq/txcript.git
```

`prepare` builds the wasm on install, so the machine needs the Rust toolchain.
Run the one-time toolchain setup, then it builds automatically:

```sh
# once per machine: wasm32 target + matching wasm-bindgen-cli
bun --cwd node_modules/txcript run setup
```

(Bun may ask you to trust the dependency before it runs `prepare`; add
`"txcript"` to `trustedDependencies` in your `package.json`.)

### Or build from a local checkout

```sh
git clone https://github.com/skillsynchq/txcript.git
cd txcript
bun run setup        # once: wasm target + wasm-bindgen-cli
bun run build        # produces ./pkg
```

Then import by path (e.g. as a sibling of your project), and wire it as a
prebuild step:

```jsonc
// your project's package.json
{
  "scripts": {
    "build:txcript": "cd ../txcript && bun run build",
    "prebuild": "bun run build:txcript"
  }
}
```

### API

```ts
import { convert, toCommon, fromCommon, harnesses } from "txcript";
// (or "../txcript/pkg/txcript.js" for a local checkout)
import { readFileSync, writeFileSync } from "node:fs";

const input = readFileSync("rollout.jsonl", "utf8");

// native -> native (e.g. a Codex rollout into Claude Code's JSONL)
writeFileSync("session.jsonl", convert(input, "codex", "claude_code"));

// canonical view, and back
const common = JSON.parse(toCommon(input, "codex"));   // { meta, messages }
const pi = fromCommon(JSON.stringify(common), "pi");

harnesses(); // ["claude_code","codex","opencode","pi","campfire","cursor","grok"]
```

Text-in / text-out: `input` is a harness's native session text (JSONL for
claude_code/codex/pi/campfire, the `opencode export` JSON for opencode, a
JSON export of Cursor's `store.db` for cursor, and a JSON bundle of the
session directory's files for grok); the result is the target's native text.
Invalid harness names or unparseable input throw a JS `Error`.

## Development

```sh
cargo test                                          # native suite
cargo test --no-default-features                    # without the SQLite store
bun run build && bun examples/convert.ts <file> <from> <to>
```
