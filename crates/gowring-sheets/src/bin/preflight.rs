//! `preflight [design-dir]` — lay all sheets over each other and list every unfilled cell and
//! every reference that does not resolve. Exit code 1 when anything blocks the build.

use gowring_sheets::{check, Severity, Sheets};
use std::path::PathBuf;

fn main() {
    let dir: PathBuf = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("design"));
    let sheets = match Sheets::load(&dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("PREFLIGHT FAILED to load {}: {e:#}", dir.display());
            std::process::exit(2);
        }
    };
    let report = check(&sheets);
    println!(
        "PREFLIGHT {}  ({} sheets, {} rows, {} cells)",
        dir.display(),
        sheets.sheets.len(),
        report.rows,
        report.cells
    );
    let mut findings = report.findings.clone();
    findings.sort_by(|a, b| (a.severity, &a.sheet, &a.row).cmp(&(b.severity, &b.sheet, &b.row)));
    for f in &findings {
        let tag = match f.severity {
            Severity::Error => "ERROR",
            Severity::Warn => "warn ",
            Severity::Info => "info ",
        };
        let place = match (&f.row, &f.column) {
            (Some(r), Some(c)) => format!("{}.{}.{}", f.sheet, r, c),
            (Some(r), None) => format!("{}.{}", f.sheet, r),
            (None, Some(c)) => format!("{}.*.{}", f.sheet, c),
            (None, None) => f.sheet.clone(),
        };
        println!("{tag} {place}: {}", f.message);
    }
    println!(
        "RESULT: {} ({} errors, {} warnings)",
        if report.is_clean() { "CLEAN" } else { "NOT CLEAN" },
        report.errors(),
        report.warnings()
    );
    std::process::exit(if report.is_clean() { 0 } else { 1 });
}
