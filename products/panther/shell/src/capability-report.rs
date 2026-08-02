// @file products/panther/shell/src/capability-report.rs
// @description Reports the effective state of foundational capabilities at startup.
// @created Diego Martín Lafuente <meerita@icloud.com>

use std::io::{self, Write};

use capability_system::CapabilityReport;

/// Writes the effective state of each foundational capability to stdout.
///
/// The output is canonical, language-neutral diagnostics, not user-facing UI
/// prose. Each line carries the capability identifier, the availability, and the
/// stable reason code, so the effective state surfaces at M1 without an in-window
/// text primitive (D6). The output is bounded by the fixed catalogue and carries
/// no secrets.
pub fn report_startup_capabilities(reports: &[CapabilityReport]) {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    // Startup diagnostics are best-effort. A closed or broken stdout must not
    // abort startup, so a write failure is intentionally ignored here.
    let _ = write_reports(&mut handle, reports);
}

fn write_reports<W: Write>(sink: &mut W, reports: &[CapabilityReport]) -> io::Result<()> {
    for report in reports {
        writeln!(
            sink,
            "capability {} availability={:?} reason={}",
            report.id().as_str(),
            report.availability(),
            report.reason_code(),
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use capability_system::{
        CapabilityDefinition, CapabilityId, CatalogueBuilder, Category, Manager, Maturity, Owner,
        PolicyInputs,
    };

    fn sample_reports() -> Vec<CapabilityReport> {
        let alpha = CapabilityDefinition {
            id: CapabilityId::new("panther.report-alpha"),
            owner: Owner::Panther,
            category: Category::ProductFeature,
            maturity: Maturity::Stable,
            dependencies: &[],
            is_mandatory: false,
            is_built: true,
        };
        let beta = CapabilityDefinition {
            id: CapabilityId::new("purr.report-beta"),
            owner: Owner::Purr,
            category: Category::EngineService,
            maturity: Maturity::Stable,
            dependencies: &[],
            is_mandatory: false,
            is_built: false,
        };

        let mut builder = CatalogueBuilder::new();
        builder.add(alpha).add(beta);
        let catalogue = builder.build().expect("the sample catalogue builds");
        let manager = Manager::new(catalogue, PolicyInputs::new());
        manager.catalogue_report()
    }

    #[test]
    fn writes_one_canonical_line_per_report() {
        let reports = sample_reports();

        let mut buffer = Vec::new();
        write_reports(&mut buffer, &reports).expect("writing to a vec succeeds");
        let output = String::from_utf8(buffer).expect("the output is valid utf8");

        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), reports.len());
        for report in &reports {
            assert!(output.contains(report.id().as_str()));
        }
    }
}
