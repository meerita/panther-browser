// @file engines/purr/graphics-wgpu/src/lib.rs
// @description Library root for the wgpu hardware backend adapter.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! wgpu hardware backend adapter.
//!
//! This crate implements the Panther `GraphicsBackend` contract with `wgpu`. It
//! is the only place `wgpu` and native GPU types appear behind the interface.
//! The engine and the product depend on `purr-graphics`, never on this crate's
//! internal types. The public surface exposes `WgpuBackend` and nothing from
//! `wgpu`.

#[path = "wgpu-backend.rs"]
mod wgpu_backend;

pub use wgpu_backend::WgpuBackend;
