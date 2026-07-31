//! Shared constants and helpers for the DOOM framebuffer-golden test (ADR-53).
//!
//! The oracle (`cargo xtask update-doom-golden`, which embeds the wasmtime crate
//! — kept out of this crate's own dependency tree) and the per-backend drivers
//! (the language glue below) must agree on exactly one driving contract: a
//! synthetic clock self-advancing [`DOOM_CLOCK_STEP_MS`] ms per read, [`DOOM_TICKS`]
//! `tickGame` calls, no input. The frame is then a deterministic, backend-independent
//! function of that schedule (DOOM's renderer is fixed-point integer, ADR-2), so
//! every backend and the wasmtime oracle produce byte-identical pixels.
//!
//! The golden is a P6 PPM ([`frame_to_ppm`]) — the alpha byte of the module's
//! `B,G,R,A` framebuffer is padding and is dropped, matching the demo frontends'
//! own screenshot writers (`examples/doom/ruby/main.rb`).

use std::path::{Path, PathBuf};

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::glue::fill;

/// The framebuffer this pinned `doom.wasm` renders (a 2× upscale of DOOM's
/// native 320×200); `loading.onGameInit` reports it at run time, and the golden
/// is captured at these dimensions.
pub const DOOM_FRAME_W: u32 = 640;
pub const DOOM_FRAME_H: u32 = 400;

/// Ms the synthetic clock advances **per call** to `timeInMilliseconds`. A
/// self-advancing counter (not a per-tick value) is what keeps the run
/// deterministic *and* terminating: DOOM's startup and inter-tic waits spin on
/// the clock, so a value frozen between host steps would hang forever; making
/// every read move time forward guarantees those spins exit, while the read
/// count — and thus the exact clock sequence — stays a pure function of the
/// wasm, identical across the oracle and every backend.
pub const DOOM_CLOCK_STEP_MS: i64 = 1;

/// Number of `tickGame` calls before the frame is captured. Kept minimal — two
/// ticks already clear DOOM's startup to a non-degenerate frame (the oracle
/// asserts the colour count) — because each tick is ~tens of seconds under Bash,
/// so every extra tick is real wall time on the ultra_heavy tier; pinned by the
/// golden.
pub const DOOM_TICKS: u32 = 2;

/// The cached `doom.wasm` (populated by `examples/apps/scripts/doom.sh`).
pub fn doom_wasm_path() -> PathBuf {
    crate::fixtures::apps_cache_dir().join("doom.wasm")
}

/// `examples/doom/golden/frame.ppm`, the checked-in framebuffer golden.
pub fn doom_frame_golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/doom/golden/frame.ppm")
}

/// Encode a `B,G,R,A` framebuffer (row-major, 4 bytes/pixel, alpha padding) as a
/// binary P6 PPM, dropping the alpha byte. The exact byte layout the per-backend
/// glue must reproduce on stdout for the golden comparison.
pub fn frame_to_ppm(frame: &[u8], w: u32, h: u32) -> Vec<u8> {
    assert_eq!(
        frame.len(),
        (w * h * 4) as usize,
        "framebuffer size mismatch"
    );
    let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
    out.reserve((w * h * 3) as usize);
    for px in frame.chunks_exact(4) {
        // memory order is B,G,R,A → PPM wants R,G,B; A is padding, dropped.
        out.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    out
}

/// Convert `doom.wasm` to library mode with `lang`, append `glue` that drives
/// the deterministic contract and writes the frame as a P6 PPM to stdout, and
/// require it byte-identical to the golden. The `{ticks}`/`{clock_step}`
/// placeholders in `glue` are filled from [`DOOM_TICKS`]/[`DOOM_CLOCK_STEP_MS`]
/// so the driving constants live in one place. Ultra-tier: heavy (ADR-53).
pub fn run_doom_frame_case(lang: &dyn BackendUnderTest, glue: &str) {
    let bytes = read_doom_wasm();
    let class = lang.convert_app(&bytes, Mode::Library, "doom");
    let glue = fill(
        glue,
        &[
            ("ticks", &DOOM_TICKS.to_string()),
            ("clock_step", &DOOM_CLOCK_STEP_MS.to_string()),
        ],
    );
    let output = lang.run(&format!("{class}\n{glue}"), &[], "");
    assert!(
        output.status.success(),
        "doom frame under {}: nonzero exit {}\n{}",
        lang.name(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let golden = std::fs::read(doom_frame_golden_path())
        .expect("read doom frame golden — regenerate with `cargo xtask update-doom-golden`");
    assert!(
        output.stdout == golden,
        "doom frame under {}: rendered frame differs from the golden ({} vs {} golden bytes)\nstderr: {}",
        lang.name(),
        output.stdout.len(),
        golden.len(),
        String::from_utf8_lossy(&output.stderr)
    );
    println!(
        "doom frame under {}: matches golden ({} bytes)",
        lang.name(),
        golden.len()
    );
}

/// Read the cached `doom.wasm`, failing loud (ADR-15) when it is absent.
fn read_doom_wasm() -> Vec<u8> {
    let wasm = doom_wasm_path();
    assert!(
        wasm.exists(),
        "doom not cached — run examples/apps/scripts/doom.sh (see docs/testing.md)"
    );
    std::fs::read(&wasm).expect("read doom.wasm")
}
