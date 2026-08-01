// @file engines/purr/graphics/src/backend-selection.rs
// @description Maps acceleration policy and hardware availability to a backend.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Backend selection.
//!
//! This maps two runtime facts to a `BackendKind`: whether the
//! `purr.gpu-acceleration` capability resolved to enabled, and whether the
//! platform hardware is available. The hardware backend is chosen only when both
//! hold. Every other combination selects the software backend, which always
//! renders. This keeps the acceleration decision explicit and independent of the
//! product policy that resolves the capability.

use crate::backend::BackendKind;

/// Selects the backend from the acceleration policy and hardware availability.
///
/// Returns `Hardware` only when acceleration is enabled and the hardware is
/// available. Every other combination returns `Software`, so rendering is always
/// possible.
pub fn select_backend(gpu_acceleration_enabled: bool, hardware_available: bool) -> BackendKind {
    if gpu_acceleration_enabled && hardware_available {
        return BackendKind::Hardware;
    }

    BackendKind::Software
}

#[cfg(test)]
mod tests {
    use super::select_backend;
    use crate::backend::BackendKind;

    #[test]
    fn enabled_with_hardware_selects_hardware() {
        assert_eq!(select_backend(true, true), BackendKind::Hardware);
    }

    #[test]
    fn every_other_combination_selects_software() {
        assert_eq!(select_backend(true, false), BackendKind::Software);
        assert_eq!(select_backend(false, true), BackendKind::Software);
        assert_eq!(select_backend(false, false), BackendKind::Software);
    }
}
