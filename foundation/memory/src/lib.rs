// @file foundation/memory/src/lib.rs
// @description Library root for the shared memory foundation.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Shared memory foundation.
//!
//! This crate holds the neutral memory mechanisms that the Panther product and
//! the Purr engine share. It provides a safe generational index arena, the
//! eight-region model, and, together with the budget and accounting types, the
//! means to bound and observe per-region memory. It ships mechanism and safe
//! defaults only. The numeric budget policy is Panther policy set elsewhere.
//!
//! The [`Arena`] hands out generational [`ArenaId`] handles rather than
//! pointers. A freed slot advances its generation on reuse, so a stale handle to
//! a reused slot is rejected. This aligns with the `purr-graphics` generation
//! model. The crate uses only safe Rust and keeps `unsafe_code = "deny"`.
//!
//! [`Region`] names the eight memory regions the engine divides process memory
//! into. The short-lived regions take arena allocation; the capacity regions
//! take budgeted byte-accounting.

#[path = "arena.rs"]
mod arena;
#[path = "region.rs"]
mod region;

pub use arena::{Arena, ArenaId};
pub use region::Region;
