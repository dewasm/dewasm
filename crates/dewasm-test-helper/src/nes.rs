//! Shared constants and helpers for the NES framebuffer-snapshot test (issue #114).
//! It mirrors the DOOM one.
//! The oracle and the per-backend drivers (the language glue) must agree on one driving contract.
//! The oracle is `cargo xtask update-snapshots`, whose NES target embeds the `wasmtime` crate.
//! That crate is kept out of this crate's dependency tree.
//! The contract: load the example ROM and tick [`NES_FRAMES`] frames with **no input**.
//! Then dump the framebuffer.
//! The `agnes` emulator is deterministic (fixed-point integer, no wall clock).
//! So every backend and the Wasmtime oracle produce byte-identical pixels.
//! No synthetic clock is needed, unlike DOOM.
//!
//! "Dump the framebuffer" means `agnes`'s own representation, not a rendered image (issue #117).
//! `screenOffset()` points at `frameWidth * frameHeight` palette *indices*.
//! They are row-major, one byte per pixel.
//! `paletteOffset()` points at the fixed [`NES_PALETTE_ENTRIES`]-entry `R,G,B,A` palette.
//! So a host composes a pixel as `palette[screen[i] & 0x3f]`.
//! [`nes_frame_to_ppm`] is both the oracle's encoder and the shape every backend's glue reproduces.
//! The `& 0x3f` mask is required: indices above 63 occur.

use std::path::PathBuf;

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::glue::fill;

/// The framebuffer this NES emulator renders.
/// That is `agnes`'s fixed native resolution, `AGNES_SCREEN_WIDTH`×`AGNES_SCREEN_HEIGHT`.
/// The snapshot is captured at these dimensions.
/// `frameWidth`/`frameHeight` report them at run time.
pub const NES_FRAME_W: u32 = 256;
pub const NES_FRAME_H: u32 = 240;

/// The palette `paletteOffset` points at: 64 entries of 4 bytes (`R,G,B,A`), that is 256 bytes.
/// Fixed data, so a host reads it once.
pub const NES_PALETTE_ENTRIES: usize = 64;

/// Number of `tickGame` calls (one emulated video frame each) before the frame is captured.
/// There is no controller input.
/// It is the smallest count reaching a stable, non-degenerate screen.
/// Alter Ego boots near-black (~15 ticks) and settles into its final credits image by frame 37.
/// The image stays identical through 180+, so 40 leaves a small margin.
/// Every frame is real wall time under Bash, so smaller is better; the snapshot uses this count.
pub const NES_FRAMES: u32 = 40;

/// The cached `nes.wasm` reactor library (populated by
/// `examples/apps/scripts/nes.sh`).
pub fn nes_wasm_path() -> PathBuf {
    crate::fixtures::apps_cache_dir().join("nes.wasm")
}

/// The cached example ROM (`cache/alter_ego.nes`, populated by the same script).
pub fn alter_ego_rom_path() -> PathBuf {
    crate::fixtures::apps_cache_dir().join("alter_ego.nes")
}

/// `examples/apps/snapshots/nes_frame.ppm`, the checked-in framebuffer snapshot
/// (in the shared snapshots directory, so its stem carries the `nes_` prefix).
pub fn nes_frame_snapshot_path() -> PathBuf {
    crate::fixtures::apps_snapshot_dir().join("nes_frame.ppm")
}

/// Encode `agnes`'s own frame representation (`w * h` palette indices plus the
/// [`NES_PALETTE_ENTRIES`]-entry `R,G,B,A` palette) as a binary P6 PPM.
/// The byte layout the per-backend glue must reproduce on `stdout` for the snapshot comparison.
pub fn nes_frame_to_ppm(screen: &[u8], palette: &[u8], w: u32, h: u32) -> Vec<u8> {
    assert_eq!(
        screen.len(),
        (w * h) as usize,
        "screen buffer size mismatch"
    );
    assert_eq!(
        palette.len(),
        NES_PALETTE_ENTRIES * 4,
        "palette size mismatch"
    );
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    out.reserve((w * h * 3) as usize);
    for &ix in screen {
        let c = &palette[(ix as usize & 0x3f) * 4..];
        // palette order is R,G,B,A → PPM wants R,G,B; A is padding, dropped.
        out.extend_from_slice(&[c[0], c[1], c[2]]);
    }
    out
}

/// Convert `nes.wasm` to library mode with `lang`.
/// Append `glue` that loads the ROM, ticks the deterministic contract, and writes the frame.
/// The frame goes to `stdout` as a P6 PPM, which must be byte-identical to the snapshot.
/// The `{frames}`/`{rom}` placeholders in `glue` are filled from [`NES_FRAMES`] and the ROM path.
/// Filling them here keeps the driving constants in one place.
pub fn run_nes_frame_case(lang: &dyn BackendUnderTest, glue: &str) {
    let bytes = read_nes_wasm();
    let class = lang.convert_app(&bytes, Mode::Library, &lang.module_name("nes"));
    let glue = fill(
        glue,
        &[
            ("frames", &NES_FRAMES.to_string()),
            ("rom", &alter_ego_rom_path().to_string_lossy()),
        ],
    );
    let output = lang.run(&format!("{class}\n{glue}"), &[], "");
    assert!(
        output.status.success(),
        "nes frame under {}: nonzero exit {}\n{}",
        lang.name(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let snapshot = std::fs::read(nes_frame_snapshot_path())
        .expect("read nes frame snapshot: regenerate with `cargo xtask update-snapshots`");
    assert!(
        output.stdout == snapshot,
        "nes frame under {}: rendered frame differs from the snapshot ({} vs {} snapshot bytes)\nstderr: {}",
        lang.name(),
        output.stdout.len(),
        snapshot.len(),
        String::from_utf8_lossy(&output.stderr)
    );
    println!(
        "nes frame under {}: matches snapshot ({} bytes)",
        lang.name(),
        snapshot.len()
    );
}

/// Read the cached `nes.wasm`, failing loud when it is missing.
fn read_nes_wasm() -> Vec<u8> {
    let wasm = nes_wasm_path();
    assert!(
        wasm.exists(),
        "nes not cached: run examples/apps/scripts/nes.sh (see docs/testing.md)"
    );
    std::fs::read(&wasm).expect("read nes.wasm")
}
