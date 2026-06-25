# transcript

`transcript` is a small Rust crate for transforming AI coding-agent session
transcripts between harness formats.

Each harness has its own native transcript shape. This crate maps those shapes
through a typed common model, then re-emits them for another harness. Native
load/save stays byte-lossless; cross-harness transformation preserves the
semantic conversation: messages, reasoning, tool calls, tool results, images,
metadata, and usage where available.

Supported harnesses:

- Claude Code
- Codex
- OpenCode
- pi
- Campfire

## Example

```rust
use transcript::{ClaudeCode, Codex, convert};

let codex_session = convert::<ClaudeCode, Codex>(&claude_session)?;
```

## CLI

The `transcript` binary discovers local sessions and continues one in any
harness — the offline half of replay's `continue --local`:

```sh
transcript list                                   # local sessions, every harness
transcript continue <id> [--with <harness>]       # continue, then launch the harness
```

`continue` hands the terminal to the harness when done (on Unix it `exec`s).
`--out <dir>` / `--no-resume` write the session without launching.

## WASM (Bun / Node / browser)

The pure codec compiles to WebAssembly — the JS host owns I/O and calls in for
the transformation. The `Store` layer (filesystem, SQLite, subprocess) is native
and excluded from the WASM build.

```sh
cargo build --lib --release --target wasm32-unknown-unknown \
    --no-default-features --features wasm
wasm-bindgen target/wasm32-unknown-unknown/release/transcript.wasm \
    --out-dir pkg --target nodejs
```

```ts
import { convert, toCommon, harnesses } from "./pkg/transcript.js";

const claude = convert(codexJsonl, "codex", "claude_code"); // native -> native
const common = JSON.parse(toCommon(codexJsonl, "codex"));    // { meta, messages }
```

Text-in / text-out: `input` is a harness's native session text (JSONL, or the
`opencode export` JSON for opencode); the result is the target's native text.

## Development

```sh
cargo test
```
