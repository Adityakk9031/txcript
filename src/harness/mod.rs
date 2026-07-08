//! Per-harness implementations.
//!
//! Each harness is one flat file defining its native record types (its
//! [`Harness::Body`](crate::Harness)), its [`Codec`](crate::Codec) to and from
//! [`Common`](crate::Common), and its [`Store`](crate::Store). The core
//! compiles with none of them present.

pub mod amp;
pub mod antigravity;
pub mod campfire;
pub mod claude_code;
pub mod codex;
pub mod cursor;
pub mod grok;
pub mod opencode;
pub mod pi;

pub(crate) mod jsonl;
