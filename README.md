# txcript

`txcript` transforms AI coding-agent session transcripts between harness
formats.

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
use txcript::{ClaudeCode, Codex, Codec, TextCodec, convert};

let claude = ClaudeCode::from_text(jsonl_text)?;     // Transcript<ClaudeCode>
let codex = convert::<ClaudeCode, Codex>(&claude)?;  // Transcript<Codex>
let codex_text = Codex::to_text(&codex)?;            // native rollout JSONL
```

Or go through disk with a `Store`:

```rust
use txcript::{ClaudeStore, CodexStore, Codex, Store, convert};

let store = ClaudeStore::default_root().expect("home dir");
let found = store.discover()?;                       // cheap metadata scan
let claude = store.load(&found[0].reference)?;       // Transcript<ClaudeCode>

let codex = convert::<_, Codex>(&claude)?;
CodexStore::default_root().expect("home dir").save(&codex)?;  // resumable on disk
```

The canonical model is `Transcript<Common>` — `Meta` + `Vec<Message>`, where a
`Message` holds typed `Block`s (`Text`, `Thinking`, `ToolUse`, `ToolResult`,
`Image`) and a typed `Tool` enum.

## Use as a CLI

```sh
cargo install txcript        # installs the `txcript` binary
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

## Use as a WASM module (Bun / Node)

The pure codec compiles to WebAssembly; the JS host owns all I/O and calls in
for the transformation. The `Store` layer (filesystem, SQLite, subprocess) stays
native and is excluded from the WASM build.

### Install from git

```sh
bun add git+ssh://git@github.com/NishantJoshi00/txcript.git
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
git clone https://github.com/NishantJoshi00/txcript.git
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

harnesses(); // ["claude_code","codex","opencode","pi","campfire","cursor"]
```

Text-in / text-out: `input` is a harness's native session text (JSONL for
claude_code/codex/pi/campfire, the `opencode export` JSON for opencode, and a
JSON export of Cursor's `store.db` for cursor); the result is the target's
native text. Invalid harness names or unparseable input throw a JS `Error`.

## Development

```sh
cargo test                                          # native suite
cargo test --no-default-features                    # without the SQLite store
bun run build && bun examples/convert.ts <file> <from> <to>
```
