//! Per-harness implementations.
//!
//! Each harness is one flat file defining its native record types (its
//! [`Harness::Body`](crate::Harness)), its [`Codec`](crate::Codec) to and from
//! [`Common`](crate::Common), and its [`Store`](crate::Store). They are added
//! one at a time; the core compiles with none of them present.

// pub mod claude_code;
// pub mod codex;
// pub mod opencode;
// pub mod pi;
// pub mod campfire;
