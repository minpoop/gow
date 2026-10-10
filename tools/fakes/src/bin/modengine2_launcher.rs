// Records how it was called, then "launches the game" like the real one: spawn and return at once.
use std::io::Write;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = std::env::var("FAKE_LOG").unwrap_or_else(|_| "fake_me2.log".into());
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&out).unwrap();
    writeln!(f, "args={args:?}").unwrap();
    for k in ["GOWRING_HOME", "GOWRING_CACHE", "GOWRING_LOCAL", "GOWRING_CHANNEL", "GOWRING_SAVE"] {
        writeln!(f, "{k}={}", std::env::var(k).unwrap_or_default()).unwrap();
    }
    let p = args.iter().position(|a| a == "-p").and_then(|i| args.get(i + 1)).expect("-p <exe>");
    let c = args.iter().position(|a| a == "-c").and_then(|i| args.get(i + 1)).expect("-c <config>");
    writeln!(f, "config_exists={} text={:?}", std::path::Path::new(c).exists(), std::fs::read_to_string(c).unwrap_or_default().lines().take(6).collect::<Vec<_>>()).unwrap();
    std::process::Command::new(p).spawn().expect("spawn game");
}
