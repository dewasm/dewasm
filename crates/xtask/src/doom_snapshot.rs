//! The DOOM framebuffer-snapshot oracle: run the *original* `doom.wasm`
//! under the wasmtime crate with the deterministic driving contract, and write the rendered frame to `examples/apps/snapshots/doom_frame.ppm`. wasmtime lives here, in dev tooling, not in `dewasm-test-helper`: the per-backend comparison never needs an embedder, only the committed snapshot this produces.
//! A PNG rendering of the same frame is emitted alongside it for human inspection (the
//! DOOM README); only the PPM is the compared oracle.

use anyhow::{ensure, Context, Result};
use wasmtime::{Caller, Engine, Linker, Module, Store};

/// Host state threaded through the imports: the synthetic clock, the last framebuffer offset `ui.drawFrame` delivered, and the dimensions
/// `loading.onGameInit` reported.
struct DoomState {
    ms: i64,
    frame_off: Option<u32>,
    frame_w: u32,
    frame_h: u32,
}

/// Instantiate and drive `doom.wasm` under the deterministic contract, returning the captured framebuffer bytes (`B,G,R,A`) and its dimensions.
/// Kept in
/// `wasmtime::Result` so wasmtime's `?` composes; the caller lifts it to anyhow.
fn capture_frame(bytes: &[u8]) -> wasmtime::Result<(Vec<u8>, u32, u32)> {
    let engine = Engine::default();
    let module = Module::new(&engine, bytes)?;
    let mut store = Store::new(
        &engine,
        DoomState {
            ms: 0,
            frame_off: None,
            frame_w: 0,
            frame_h: 0,
        },
    );

    // The ten host imports under the deterministic contract: no console output, no save state, a synthetic clock, the embedded WAD (wad imports are no-ops, leaving their out-params zero), and dims/offset recorded.
    let mut linker = Linker::new(&engine);
    linker.func_wrap(
        "console",
        "onErrorMessage",
        |_: Caller<'_, DoomState>, _: i32, _: i32| {},
    )?;
    linker.func_wrap(
        "console",
        "onInfoMessage",
        |_: Caller<'_, DoomState>, _: i32, _: i32| {},
    )?;
    linker.func_wrap(
        "gameSaving",
        "writeSaveGame",
        |_: Caller<'_, DoomState>, _id: i32, _src: i32, len: i32| -> i32 { len },
    )?;
    linker.func_wrap(
        "gameSaving",
        "readSaveGame",
        |_: Caller<'_, DoomState>, _id: i32, _dst: i32| -> i32 { 0 },
    )?;
    linker.func_wrap(
        "gameSaving",
        "sizeOfSaveGame",
        |_: Caller<'_, DoomState>, _id: i32| -> i32 { 0 },
    )?;
    linker.func_wrap(
        "runtimeControl",
        "timeInMilliseconds",
        |mut caller: Caller<'_, DoomState>| -> i64 {
            let s = caller.data_mut();
            s.ms += dewasm_test_helper::DOOM_CLOCK_STEP_MS;
            s.ms
        },
    )?;
    linker.func_wrap(
        "ui",
        "drawFrame",
        |mut caller: Caller<'_, DoomState>, off: i32| {
            caller.data_mut().frame_off = Some(off as u32);
        },
    )?;
    linker.func_wrap(
        "loading",
        "readWads",
        |_: Caller<'_, DoomState>, _: i32, _: i32| {},
    )?;
    linker.func_wrap(
        "loading",
        "wadSizes",
        |_: Caller<'_, DoomState>, _: i32, _: i32| {},
    )?;
    linker.func_wrap(
        "loading",
        "onGameInit",
        |mut caller: Caller<'_, DoomState>, w: i32, h: i32| {
            let s = caller.data_mut();
            s.frame_w = w as u32;
            s.frame_h = h as u32;
        },
    )?;

    let instance = linker.instantiate(&mut store, &module)?;
    let init = instance.get_typed_func::<(), ()>(&mut store, "initGame")?;
    let tick = instance.get_typed_func::<(), ()>(&mut store, "tickGame")?;
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("doom.wasm has no `memory` export");

    // initGame (fires onGameInit), then N ticks: no key events.
    // The clock self-advances on every read, so nothing is stepped here.
    init.call(&mut store, ())?;
    for _ in 0..dewasm_test_helper::DOOM_TICKS {
        tick.call(&mut store, ())?;
    }

    let off = store
        .data()
        .frame_off
        .expect("ui.drawFrame never fired: no frame to capture") as usize;
    let (w, h) = (store.data().frame_w, store.data().frame_h);
    let frame = memory.data(&store)[off..off + (w * h * 4) as usize].to_vec();
    Ok((frame, w, h))
}

/// Recapture the DOOM framebuffer from the embedded wasmtime; returns the compared P6-PPM bytes plus a PNG for human inspection.
///
/// `update-snapshots` writes both; the per-backend test compares only the PPM.
pub fn capture_doom_frame() -> Result<(Vec<u8>, Vec<u8>)> {
    let wasm_path = dewasm_test_helper::doom_wasm_path();
    let bytes = std::fs::read(&wasm_path).with_context(|| {
        format!(
            "read {}: run examples/apps/scripts/doom.sh first",
            wasm_path.display()
        )
    })?;

    let (frame, w, h) = capture_frame(&bytes).map_err(anyhow::Error::msg)?;
    ensure!(
        w == dewasm_test_helper::DOOM_FRAME_W && h == dewasm_test_helper::DOOM_FRAME_H,
        "onGameInit reported {w}x{h}, expected {}x{} (pin bump?)",
        dewasm_test_helper::DOOM_FRAME_W,
        dewasm_test_helper::DOOM_FRAME_H
    );

    // Guard against a degenerate (blank/near-blank) capture: DOOM's paletted renderer lands in the low hundreds of distinct colors on a real frame.
    let distinct = frame
        .chunks_exact(4)
        .map(|px| [px[0], px[1], px[2]])
        .collect::<std::collections::HashSet<_>>()
        .len();
    ensure!(
        distinct > 50,
        "captured frame looks degenerate ({distinct} distinct colors): check the tick count/clock"
    );

    Ok((
        dewasm_test_helper::frame_to_ppm(&frame, w, h),
        frame_to_png(&frame, w, h)?,
    ))
}

/// Encode a `B,G,R,A` framebuffer (row-major, alpha padding dropped) as an 8-bit
/// RGB PNG.
/// Settings are the crate defaults, kept fixed so regeneration is byte-stable (verified by capturing twice and diffing).
/// This PNG exists only for humans to view: no test compares it.
fn frame_to_png(frame: &[u8], w: u32, h: u32) -> Result<Vec<u8>> {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for px in frame.chunks_exact(4) {
        rgb.extend_from_slice(&[px[2], px[1], px[0]]);
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w, h);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&rgb)?;
    writer.finish()?;
    Ok(out)
}
