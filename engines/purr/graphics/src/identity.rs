// @file engines/purr/graphics/src/identity.rs
// @description Defines identity and generation value types for the graphics protocol seam.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Identity and generation value types.
//!
//! The interface models scene, surface, resource, and device lifetimes with a
//! numeric identifier paired with a generation. A generation lets a reused
//! numeric identifier not alias an old lifetime, so a stale scene, surface, or
//! resource can be rejected without mutating active state.
//!
//! These are data-only value types. No backend type appears here.

/// Isolates the identifier space of one submission producer.
///
/// Two producers can allocate the same numeric identifier without collision
/// because the namespace distinguishes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ProducerNamespace(u32);

impl ProducerNamespace {
    pub fn new(value: u32) -> Self {
        Self(value)
    }

    pub fn value(self) -> u32 {
        self.0
    }
}

/// Marks the lifetime of one graphics device.
///
/// A device loss advances this generation. A resource stamped with an older
/// device generation is stale and is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceGeneration(u64);

impl DeviceGeneration {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }

    /// Returns the next generation.
    ///
    /// Uses checked arithmetic. A `u64` generation cannot overflow in practice,
    /// so `None` is unreachable, but the interface fails safe instead of
    /// panicking.
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// Identifies one scene within a producer namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SceneId(u64);

impl SceneId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

/// Marks the lifetime of one scene identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SceneGeneration(u64);

impl SceneGeneration {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }

    /// Returns the next generation.
    ///
    /// Uses checked arithmetic. A `u64` generation cannot overflow in practice,
    /// so `None` is unreachable, but the interface fails safe instead of
    /// panicking.
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// Identifies one surface within a producer namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceId(u64);

impl SurfaceId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

/// Marks the lifetime of one surface identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceGeneration(u64);

impl SurfaceGeneration {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }

    /// Returns the next generation.
    ///
    /// Uses checked arithmetic. A `u64` generation cannot overflow in practice,
    /// so `None` is unreachable, but the interface fails safe instead of
    /// panicking.
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// Identifies one resource within a producer namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceId(u64);

impl ResourceId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

/// Marks the lifetime of one resource identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceGeneration(u64);

impl ResourceGeneration {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }

    /// Returns the next generation.
    ///
    /// Uses checked arithmetic. A `u64` generation cannot overflow in practice,
    /// so `None` is unreachable, but the interface fails safe instead of
    /// panicking.
    pub fn next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

/// Kind of a graphics resource.
///
/// A closed, small set. `GlyphAtlas` and `Tile` are reserved for later phases
/// and are not produced by the M0 operation set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceKind {
    Texture,
    Buffer,
    RenderTarget,
    /// Reserved for a later text-rendering phase.
    GlyphAtlas,
    /// Reserved for a later tiled-composition phase.
    Tile,
}

/// Identifies one frame submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FrameToken(u64);

impl FrameToken {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

/// Full identity of a surface, stable across a surface generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceIdentity {
    surface_id: SurfaceId,
    surface_generation: SurfaceGeneration,
    producer_namespace: ProducerNamespace,
}

impl SurfaceIdentity {
    pub fn new(
        surface_id: SurfaceId,
        surface_generation: SurfaceGeneration,
        producer_namespace: ProducerNamespace,
    ) -> Self {
        Self {
            surface_id,
            surface_generation,
            producer_namespace,
        }
    }

    pub fn surface_id(self) -> SurfaceId {
        self.surface_id
    }

    pub fn surface_generation(self) -> SurfaceGeneration {
        self.surface_generation
    }

    pub fn producer_namespace(self) -> ProducerNamespace {
        self.producer_namespace
    }
}

/// Full identity of a graphics resource, stable across a resource generation
/// and tied to the device generation that created it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GpuResourceIdentity {
    producer_namespace: ProducerNamespace,
    resource_id: ResourceId,
    resource_generation: ResourceGeneration,
    resource_kind: ResourceKind,
    device_generation: DeviceGeneration,
}

impl GpuResourceIdentity {
    pub fn new(
        producer_namespace: ProducerNamespace,
        resource_id: ResourceId,
        resource_generation: ResourceGeneration,
        resource_kind: ResourceKind,
        device_generation: DeviceGeneration,
    ) -> Self {
        Self {
            producer_namespace,
            resource_id,
            resource_generation,
            resource_kind,
            device_generation,
        }
    }

    pub fn producer_namespace(self) -> ProducerNamespace {
        self.producer_namespace
    }

    pub fn resource_id(self) -> ResourceId {
        self.resource_id
    }

    pub fn resource_generation(self) -> ResourceGeneration {
        self.resource_generation
    }

    pub fn resource_kind(self) -> ResourceKind {
        self.resource_kind
    }

    pub fn device_generation(self) -> DeviceGeneration {
        self.device_generation
    }
}

/// Full identity of a scene, tied to the surface it targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SceneIdentity {
    scene_id: SceneId,
    scene_generation: SceneGeneration,
    surface_id: SurfaceId,
    surface_generation: SurfaceGeneration,
}

impl SceneIdentity {
    pub fn new(
        scene_id: SceneId,
        scene_generation: SceneGeneration,
        surface_id: SurfaceId,
        surface_generation: SurfaceGeneration,
    ) -> Self {
        Self {
            scene_id,
            scene_generation,
            surface_id,
            surface_generation,
        }
    }

    pub fn scene_id(self) -> SceneId {
        self.scene_id
    }

    pub fn scene_generation(self) -> SceneGeneration {
        self.scene_generation
    }

    pub fn surface_id(self) -> SurfaceId {
        self.surface_id
    }

    pub fn surface_generation(self) -> SurfaceGeneration {
        self.surface_generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_advance_is_strictly_greater() {
        let current = ResourceGeneration::new(7);
        let advanced = current.next().expect("u64 generation does not overflow");

        assert!(advanced.value() > current.value());
    }

    #[test]
    fn same_resource_number_with_different_generation_differs() {
        let namespace = ProducerNamespace::new(1);
        let resource_id = ResourceId::new(42);
        let device_generation = DeviceGeneration::new(3);

        let first = GpuResourceIdentity::new(
            namespace,
            resource_id,
            ResourceGeneration::new(1),
            ResourceKind::Texture,
            device_generation,
        );
        let second = GpuResourceIdentity::new(
            namespace,
            resource_id,
            ResourceGeneration::new(2),
            ResourceKind::Texture,
            device_generation,
        );

        assert_ne!(first, second);
    }

    #[test]
    fn accessors_round_trip_constructor_inputs() {
        let identity = GpuResourceIdentity::new(
            ProducerNamespace::new(5),
            ResourceId::new(11),
            ResourceGeneration::new(2),
            ResourceKind::RenderTarget,
            DeviceGeneration::new(9),
        );

        assert_eq!(identity.producer_namespace().value(), 5);
        assert_eq!(identity.resource_id().value(), 11);
        assert_eq!(identity.resource_generation().value(), 2);
        assert_eq!(identity.resource_kind(), ResourceKind::RenderTarget);
        assert_eq!(identity.device_generation().value(), 9);
    }
}
