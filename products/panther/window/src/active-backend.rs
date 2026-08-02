// @file products/panther/window/src/active-backend.rs
// @description Probes hardware, selects a backend, and dispatches to the chosen one.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Backend probe, selection, and dispatch for the windowing layer.
//!
//! The `GraphicsBackend` contract is `Sized`, so it is not object-safe. The two
//! concrete backends are held in a closed enum and dispatched by hand, which
//! keeps the backend type out of the windowing seam: callers pass Panther
//! identities and descriptors only.
//!
//! The probe tries to build the hardware backend. `select_backend` then maps the
//! probe result and the M0 acceleration default to a `BackendKind`. When no
//! usable adapter exists, the result is always the software backend, which always
//! renders.

use purr_graphics::{
    BackendKind, Extent2d, FrameSubmission, GpuResourceIdentity, GraphicsBackend, GraphicsError,
    PresentationTargetDescriptor, SurfaceIdentity, TextureDescriptor, WindowSurface,
    select_backend,
};
use purr_graphics_software::SoftwareBackend;
use purr_graphics_wgpu::WgpuBackend;

use crate::window_error::WindowError;

/// M0 default for the `purr.gpu-acceleration` capability.
///
/// The product policy source that resolves the capability is a later integration.
/// At M0 acceleration defaults to enabled, so the hardware backend is used
/// whenever the probe finds a usable adapter.
const GPU_ACCELERATION_ENABLED_DEFAULT: bool = true;

/// The backend chosen at runtime, holding one concrete backend.
pub(crate) enum ActiveBackend {
    Hardware(WgpuBackend),
    Software(SoftwareBackend),
}

impl ActiveBackend {
    pub(crate) fn create_presentation_target(
        &mut self,
        surface: WindowSurface<'_>,
        descriptor: PresentationTargetDescriptor,
    ) -> Result<SurfaceIdentity, GraphicsError> {
        match self {
            Self::Hardware(backend) => backend.create_presentation_target(surface, descriptor),
            Self::Software(backend) => backend.create_presentation_target(surface, descriptor),
        }
    }

    pub(crate) fn resize_presentation_target(
        &mut self,
        surface: SurfaceIdentity,
        extent: Extent2d,
    ) -> Result<(), GraphicsError> {
        match self {
            Self::Hardware(backend) => backend.resize_presentation_target(surface, extent),
            Self::Software(backend) => backend.resize_presentation_target(surface, extent),
        }
    }

    pub(crate) fn allocate_texture(
        &mut self,
        descriptor: &TextureDescriptor,
    ) -> Result<GpuResourceIdentity, GraphicsError> {
        match self {
            Self::Hardware(backend) => backend.allocate_texture(descriptor),
            Self::Software(backend) => backend.allocate_texture(descriptor),
        }
    }

    pub(crate) fn submit(
        &mut self,
        surface: SurfaceIdentity,
        submission: &FrameSubmission,
    ) -> Result<(), GraphicsError> {
        match self {
            Self::Hardware(backend) => backend.submit(surface, submission),
            Self::Software(backend) => backend.submit(surface, submission),
        }
    }

    pub(crate) fn present(&mut self, surface: SurfaceIdentity) -> Result<(), GraphicsError> {
        match self {
            Self::Hardware(backend) => backend.present(surface),
            Self::Software(backend) => backend.present(surface),
        }
    }
}

/// Chooses the backend kind from a hardware-probe result.
///
/// This is the windowing-layer fallback decision. It feeds the M0 acceleration
/// default and the probe outcome to the engine selection helper. When the probe
/// found no usable adapter, the result is always `Software`.
fn choose_backend_kind(hardware_available: bool) -> BackendKind {
    select_backend(GPU_ACCELERATION_ENABLED_DEFAULT, hardware_available)
}

/// Probes for a hardware backend and returns the selected active backend.
///
/// The probe tries to build the `wgpu` backend. Success means a usable adapter
/// exists, so the acceleration default selects it. Any failure falls back to the
/// software backend, which always renders.
pub(crate) fn create_active_backend() -> Result<ActiveBackend, WindowError> {
    let probe = WgpuBackend::create(BackendKind::Hardware).ok();

    if choose_backend_kind(probe.is_some()) == BackendKind::Hardware
        && let Some(hardware) = probe
    {
        return Ok(ActiveBackend::Hardware(hardware));
    }

    let software = SoftwareBackend::create(BackendKind::Software).map_err(WindowError::Backend)?;
    Ok(ActiveBackend::Software(software))
}

#[cfg(test)]
mod tests {
    use super::choose_backend_kind;
    use purr_graphics::BackendKind;

    #[test]
    fn probe_unavailable_falls_back_to_software() {
        assert_eq!(choose_backend_kind(false), BackendKind::Software);
    }

    #[test]
    fn probe_available_selects_hardware() {
        assert_eq!(choose_backend_kind(true), BackendKind::Hardware);
    }
}
