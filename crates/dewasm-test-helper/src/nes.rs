//! Shared constants and helpers for the NES framebuffer-snapshot test (issue
//! #114), mirroring the DOOM one (ADR-53). The oracle (`cargo xtask
//! update-snapshots`, whose NES target embeds the wasmtime crate — kept out of
//! this crate's dependency tree) and the per-backend drivers (the language glue)
//! must agree on one driving contract: load the pinned ROM, tick [`NES_FRAMES`]
//! frames with **no input**, then dump the framebuffer. agnes's emulation is
//! deterministic (fixed-point integer, no wall clock), so every backend and the
//! wasmtime oracle produce byte-identical pixels — no synthetic clock is needed,
//! unlike DOOM.
//!
//! "Dump the framebuffer" means agnes's own representation, not a rendered
//! image (issue #117): `screenOffset()` points at `frameWidth * frameHeight`
//! palette *indices* (row-major, one byte per pixel) and `paletteOffset()` at
//! the fixed [`NES_PALETTE_ENTRIES`]-entry `R,G,B,A` palette, so a host composes
//! a pixel as `palette[screen[i] & 0x3f]` — see [`nes_frame_to_ppm`], which is
//! both the oracle's encoder and the shape every backend's glue reproduces. The
//! `& 0x3f` mask is load-bearing: indices above 63 occur.

use std::path::PathBuf;

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::glue::fill;

/// The framebuffer this NES emulator renders (agnes's fixed native resolution,
/// `AGNES_SCREEN_WIDTH`×`AGNES_SCREEN_HEIGHT`); the snapshot is captured at these
/// dimensions and `frameWidth`/`frameHeight` report them at run time.
pub const NES_FRAME_W: u32 = 256;
pub const NES_FRAME_H: u32 = 240;

/// The palette `paletteOffset` points at: 64 entries of 4 bytes (`R,G,B,A`),
/// i.e. 256 bytes. Fixed data, so a host reads it once.
pub const NES_PALETTE_ENTRIES: usize = 64;

/// Number of `tickGame` calls (one emulated video frame each) before the frame
/// is captured, with no controller input. Chosen empirically as the smallest
/// count that clears the ROM's boot to a clearly recognizable, non-degenerate
/// screen: Alter Ego opens on a near-black boot frame (1 color at ~15 ticks),
/// then fades in a credits screen that reaches its final, stable image by frame
/// 37 (7 distinct colors, identical through 180+). 40 sits just inside that
/// stable region with a small margin. Every frame is real wall time on the Bash
/// backend later, so smaller is better — but a boring near-black frame is worse
/// than a handful of extra ticks, so this trades ~3 ticks of margin for a solidly
/// drawn screen. Pinned by the snapshot.
pub const NES_FRAMES: u32 = 40;

/// The cached `nes.wasm` reactor library (populated by
/// `examples/apps/scripts/nes.sh`).
pub fn nes_wasm_path() -> PathBuf {
    crate::fixtures::apps_cache_dir().join("nes.wasm")
}

/// The cached demo ROM (`cache/alter_ego.nes`, populated by the same script).
pub fn alter_ego_rom_path() -> PathBuf {
    crate::fixtures::apps_cache_dir().join("alter_ego.nes")
}

/// `examples/apps/snapshots/nes_frame.ppm`, the checked-in framebuffer snapshot
/// (in the shared snapshots dir, so its stem carries the `nes_` prefix).
pub fn nes_frame_snapshot_path() -> PathBuf {
    crate::fixtures::apps_snapshot_dir().join("nes_frame.ppm")
}

/// Encode agnes's own frame representation — `w * h` palette indices plus the
/// [`NES_PALETTE_ENTRIES`]-entry `R,G,B,A` palette — as a binary P6 PPM. The
/// exact byte layout the per-backend glue must reproduce on stdout for the
/// snapshot comparison, mask included: `palette[index & 0x3f]` (indices above
/// 63 occur, and agnes's own accessor masks them).
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

/// Convert `nes.wasm` to library mode with `lang`, append `glue` that loads the
/// ROM, ticks the deterministic contract, and writes the frame as a P6 PPM to
/// stdout, and require it byte-identical to the snapshot. The `{frames}`/`{rom}`
/// placeholders in `glue` are filled from [`NES_FRAMES`] and the cached ROM path
/// so the driving constants live in one place.
pub fn run_nes_frame_case(lang: &dyn BackendUnderTest, glue: &str) {
    let bytes = read_nes_wasm();
    let class = lang.convert_app(&bytes, Mode::Library, "nes");
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
        .expect("read nes frame snapshot — regenerate with `cargo xtask update-snapshots`");
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

/// Read the cached `nes.wasm`, failing loud (ADR-15) when it is absent.
fn read_nes_wasm() -> Vec<u8> {
    let wasm = nes_wasm_path();
    assert!(
        wasm.exists(),
        "nes not cached — run examples/apps/scripts/nes.sh (see docs/testing.md)"
    );
    std::fs::read(&wasm).expect("read nes.wasm")
}
