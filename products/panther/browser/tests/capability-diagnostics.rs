// @file products/panther/browser/tests/capability-diagnostics.rs
// @description Integration tests for explainability, the orthogonal axes, and the downward snapshot.
// @created Diego Martín Lafuente <meerita@icloud.com>

//! Asserts the cross-cutting invariants of the assembled system: every state is
//! explainable through enum fields and a derived message, an unavailable
//! capability never carries a runtime lifecycle, and the downward engine-policy
//! snapshot matches the resolved availability of every engine capability.

use capability_system::{Availability, DecidingAuthority, Reason};
use panther_browser::bootstrap;
use purr_embedding::WEBGPU;

#[test]
fn an_unavailable_capability_reports_its_reason_authority_and_message() {
    let result = bootstrap().expect("the built-in catalogue should build");

    let report = result
        .manager
        .report(WEBGPU)
        .expect("webgpu is in the catalogue");
    assert_eq!(report.availability(), Availability::Unsupported);
    assert_eq!(report.reason(), Reason::PlatformUnsupported);
    assert_eq!(report.authority(), DecidingAuthority::PlatformSupport);
    assert!(!report.message().is_empty());
}

#[test]
fn no_unavailable_capability_carries_a_runtime_lifecycle() {
    let result = bootstrap().expect("the built-in catalogue should build");

    for report in result.reports() {
        if report.availability() != Availability::Available {
            assert_eq!(report.lifecycle(), None);
        }
    }
}

#[test]
fn the_downward_snapshot_matches_the_resolved_engine_availability() {
    let result = bootstrap().expect("the built-in catalogue should build");
    let snapshot = result.engine_policy.diagnostics();

    assert_eq!(snapshot.entries().len(), 4);
    for entry in snapshot.entries() {
        assert_eq!(entry.id().owner_namespace(), "purr");

        let report = result
            .manager
            .report(entry.id())
            .expect("every snapshot entry is in the catalogue");
        assert_eq!(entry.availability(), report.availability());
        assert_eq!(entry.reason(), report.reason());
        assert_eq!(entry.authority(), report.authority());
    }

    let webgpu = snapshot
        .get(WEBGPU)
        .expect("webgpu appears in the engine snapshot");
    assert_eq!(webgpu.availability(), Availability::Unsupported);
}
