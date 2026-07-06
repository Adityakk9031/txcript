//! A typed intermediate representation for AI coding-agent session transcripts.
//!
//! Claude Code, Codex, `OpenCode`, and pi all record the same thing — a sequence
//! of user/assistant turns carrying text, reasoning, tool calls, tool results,
//! and images — in mutually incompatible on-disk formats. This crate models
//! that shared shape once, as [`Transcript<Common>`], and gives each harness a
//! [`Codec`] to and from it. Converting a session from one harness to another
//! is then [`convert::<A, B>`](convert): `A` → [`Common`] → `B`.
//!
//! # The two-layer fidelity contract
//!
//! - **Native ↔ disk is byte-lossless.** Each harness keeps a faithful typed
//!   representation of its records ([`Harness::Body`]); a [`Store`] round-trips
//!   it to disk without loss. This is what a same-harness resume uses.
//! - **Through [`Common`] is semantically lossless.** [`Codec::to_common`] may
//!   canonicalize representation so a thread is functional in another harness,
//!   but it never discards detail: anything a same-harness round-trip needs is
//!   preserved in [`Common`]'s typed fields. It is not byte-exact, by design —
//!   canonicalization and byte-faithfulness pull in opposite directions, and
//!   byte-faithfulness already lives at the native ↔ disk layer.
//!
//! # Shape
//!
//! - [`common`] — the canonical model ([`common::Message`], [`common::Block`],
//!   [`common::Tool`], …).
//! - [`Transcript`], [`Harness`], [`Codec`], [`Store`] — the generic type and
//!   the traits over it.
//! - [`harness`] — one module per implemented harness.

pub mod common;
pub mod error;
pub mod harness;
#[cfg(not(target_arch = "wasm32"))]
pub mod local;
#[cfg(feature = "search")]
pub mod search;
mod transcript;

#[cfg(feature = "wasm")]
mod wasm;

// The core generic API lives in the private `transcript` module, so the crate
// root is its canonical home. The concrete model and per-harness types keep
// their own module homes — reach them through [`common`] and [`harness`]
// rather than flattened at the root.
pub use error::{Error, Result};
pub use transcript::{
    Codec, Common, Discovered, Harness, HarnessId, Saved, Store, TextCodec, Transcript, convert,
};
