use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let design = manifest.join("../../design");
    println!("cargo:rerun-if-changed={}", design.display());
    let sheets = gowring_sheets::Sheets::load(&design).expect("load design sheets");
    let report = gowring_sheets::check(&sheets);
    if !report.is_clean() {
        for f in report.findings.iter().filter(|f| f.severity == gowring_sheets::Severity::Error) {
            eprintln!("preflight: {}.{}.{}: {}", f.sheet, f.row.as_deref().unwrap_or("*"), f.column.as_deref().unwrap_or("*"), f.message);
        }
        panic!("preflight is not clean: fix the sheets before building (cargo run -p gowring-sheets --bin preflight)");
    }
    let code = gowring_sheets::codegen::generate(&sheets);
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("sheets_gen.rs");
    std::fs::write(out, code).unwrap();
}
