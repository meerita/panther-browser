// @file engines/purr/graphics-software/src/lib.rs
// @description Library root for the software (CPU) backend adapter.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Software (CPU) backend adapter.
//!
//! `SoftwareBackend` implements the Panther `GraphicsBackend` contract on the CPU
//! with no GPU dependency. It is the deterministic reference backend: identical
//! inputs produce identical framebuffer bytes on every platform, so it drives the
//! pixel tests and provides the GPU-less fallback path.
//!
//! Isolation rule: this crate depends on `purr-graphics` and, for the on-screen
//! present only, on `softbuffer`. It names no `wgpu`, `winit`, or native window
//! type; `softbuffer` reaches the window through the neutral seam handle. The
//! public surface exposes `SoftwareBackend` and speaks only in Panther
//! identities, descriptors, and `GraphicsError`.

#[path = "software-backend.rs"]
mod software_backend;

pub use software_backend::SoftwareBackend;
