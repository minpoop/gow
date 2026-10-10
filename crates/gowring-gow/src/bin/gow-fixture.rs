//! Writes a tiny synthetic "God of War Ragnarök" folder for end-to-end tests of the launcher.
//! It contains nothing from the real game: a one-entry WAD, an audiopack, a bank and a test DDS.

use gowring_gow::{audiopack, bnk, dds, wad};
use std::path::PathBuf;

fn main() {
    let root: PathBuf = std::env::args().nth(1).expect("usage: gow-fixture <dir>").into();
    std::fs::create_dir_all(root.join("exec/wad")).unwrap();
    std::fs::create_dir_all(root.join("audio")).unwrap();
    std::fs::write(root.join("GoWR.exe"), b"MZ-fixture").unwrap();
    std::fs::write(
        root.join("exec/wad/ui.wad"),
        wad::build(&[("tex_hud_health_frame", 0x80A1, dds::build_test_bc1()), ("tex_hud_rage_frame", 0x80A1, vec![7u8; 64])]),
    )
    .unwrap();
    let (toc, pack) = audiopack::build_pack(&[(101, audiopack::fake_pcm_wem(480, 48000)), (102, audiopack::fake_pcm_wem(960, 48000))]);
    std::fs::write(root.join("audio/vo_en.audiopack.toc"), toc).unwrap();
    std::fs::write(root.join("audio/vo_en.0.audiopack"), pack).unwrap();
    std::fs::write(root.join("audio/Init.bnk"), bnk::build_test_bank()).unwrap();
    println!("fixture written to {}", root.display());
}
