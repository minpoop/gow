fn main() {
    let out = std::env::var("FAKE_LOG").unwrap_or_else(|_| "fake_me2.log".into());
    let _ = std::fs::OpenOptions::new().create(true).append(true).open(&out).map(|mut f| {
        use std::io::Write;
        writeln!(f, "game started").unwrap();
    });
    std::thread::sleep(std::time::Duration::from_secs(4));
}
