//! The NES framebuffer-snapshot oracle (issue #114), mirroring the DOOM one
//! (ADR-53): run the *original* `nes.wasm` under the wasmtime crate with the
//! deterministic driving contract (load the pinned ROM, tick [`NES_FRAMES`]
//! frames with no input) and write the rendered frame to
//! `examples/apps/snapshots/nes_frame.ppm`. wasmtime lives here, in dev tooling,
//! not in `dewasm-test-helper`. A PNG rendering is emitted alongside for human
//! inspection; only the PPM is the compared oracle.
//!
//! Unlike DOOM, `nes.wasm` has *no* imports (verified by nes.sh): it is a plain
//! reactor library, so the oracle only calls `_initialize` plus the demo
//! exports. The ROM is copied into guest memory through `allocRom`.

use anyhow::{ensure, Context, Result};
use dewasm_test_helper::{
    alter_ego_rom_path, nes_frame_to_ppm, nes_wasm_path, NES_FRAMES, NES_FRAME_H, NES_FRAME_W,
    NES_PALETTE_ENTRIES,
};
use wasmtime::{Engine, Instance, Linker, Module, Store};

/// Instantiate and drive `nes.wasm` under the deterministic contract, returning
/// agnes's own frame representation — the palette-index screen buffer, the
/// palette, and the frame dimensions.
fn capture_frame(bytes: &[u8], rom: &[u8]) -> wasmtime::Result<(Vec<u8>, Vec<u8>, u32, u32)> {
    let engine = Engine::default();
    let module = Module::new(&engine, bytes)?;
    let mut store = Store::new(&engine, ());
    // No imports (nes.wasm's import section is empty), so an empty linker suffices.
    let linker = Linker::new(&engine);
    let instance = linker.instantiate(&mut store, &module)?;

    // Reactor init before any other export (nothing else runs it without WASI).
    instance
        .get_typed_func::<(), ()>(&mut store, "_initialize")?
        .call(&mut store, ())?;

    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("nes.wasm has no `memory` export");
    let alloc_rom = instance.get_typed_func::<i32, i32>(&mut store, "allocRom")?;
    let init_game = instance.get_typed_func::<(), i32>(&mut store, "initGame")?;
    let tick = instance.get_typed_func::<(), ()>(&mut store, "tickGame")?;

    // Allocate a ROM buffer in guest memory, copy the iNES bytes in, then load.
    let rom_ptr = alloc_rom.call(&mut store, rom.len() as i32)? as usize;
    memory.data_mut(&mut store)[rom_ptr..rom_ptr + rom.len()].copy_from_slice(rom);
    let ok = init_game.call(&mut store, ())?;
    if ok != 1 {
        return Err(wasmtime::Error::msg(format!(
            "initGame returned {ok} (ROM load failed)"
        )));
    }

    // N frames, no input.
    for _ in 0..NES_FRAMES {
        tick.call(&mut store, ())?;
    }

    read_frame(&mut store, &instance, memory)
}

/// Read `frameWidth`/`frameHeight`/`screenOffset`/`paletteOffset` and slice the
/// index buffer and the palette out of guest memory. Split out so `memory` is
/// not borrowed across the export calls above.
fn read_frame(
    store: &mut Store<()>,
    instance: &Instance,
    memory: wasmtime::Memory,
) -> wasmtime::Result<(Vec<u8>, Vec<u8>, u32, u32)> {
    let w = instance
        .get_typed_func::<(), i32>(&mut *store, "frameWidth")?
        .call(&mut *store, ())? as u32;
    let h = instance
        .get_typed_func::<(), i32>(&mut *store, "frameHeight")?
        .call(&mut *store, ())? as u32;
    let screen_off = instance
        .get_typed_func::<(), i32>(&mut *store, "screenOffset")?
        .call(&mut *store, ())? as usize;
    let palette_off = instance
        .get_typed_func::<(), i32>(&mut *store, "paletteOffset")?
        .call(&mut *store, ())? as usize;
    let data = memory.data(&*store);
    let screen = data[screen_off..screen_off + (w * h) as usize].to_vec();
    let palette = data[palette_off..palette_off + NES_PALETTE_ENTRIES * 4].to_vec();
    Ok((screen, palette, w, h))
}

/// Recapture the NES framebuffer from a live (embedded) wasmtime and return both
/// renderings: the P6 PPM bytes for `examples/apps/snapshots/nes_frame.ppm` (the
/// compared oracle) and a PNG of the same frame for `nes_frame.png` (human
/// inspection — never compared by a test).
pub fn capture_nes_frame() -> Result<(Vec<u8>, Vec<u8>)> {
    let wasm_path = nes_wasm_path();
    let bytes = std::fs::read(&wasm_path).with_context(|| {
        format!(
            "read {} — run examples/apps/scripts/nes.sh first",
            wasm_path.display()
        )
    })?;
    let rom_path = alter_ego_rom_path();
    let rom = std::fs::read(&rom_path).with_context(|| {
        format!(
            "read {} — run examples/apps/scripts/nes.sh first",
            rom_path.display()
        )
    })?;

    let (screen, palette, w, h) = capture_frame(&bytes, &rom).map_err(anyhow::Error::msg)?;
    ensure!(
        w == NES_FRAME_W && h == NES_FRAME_H,
        "frame is {w}x{h}, expected {NES_FRAME_W}x{NES_FRAME_H} (pin bump?)"
    );

    // Guard against a degenerate (blank/near-blank) capture. NES palettes are
    // tiny, so the DOOM oracle's >50 threshold is wrong here — the Alter Ego
    // credits screen this pins measures 7 distinct colors, while the near-black
    // boot frame is a single color. A >4 threshold cleanly separates the two.
    // Counted over colors, not raw indices: distinct indices can alias onto one
    // palette entry (the mask, plus repeated black entries).
    let ppm = nes_frame_to_ppm(&screen, &palette, w, h);
    let distinct = screen
        .iter()
        .map(|&ix| &palette[(ix as usize & 0x3f) * 4..(ix as usize & 0x3f) * 4 + 3])
        .collect::<std::collections::HashSet<_>>()
        .len();
    ensure!(
        distinct > 4,
        "captured frame looks degenerate ({distinct} distinct colors) — check NES_FRAMES"
    );

    Ok((ppm, frame_to_png(&screen, &palette, w, h)?))
}

/// Encode the palette-index screen buffer as an 8-bit RGB PNG, composing pixels
/// the same way [`nes_frame_to_ppm`] does. Settings are the crate defaults, kept
/// fixed so regeneration is byte-stable. This PNG is a human-facing sidecar only
/// — no test compares it.
fn frame_to_png(screen: &[u8], palette: &[u8], w: u32, h: u32) -> Result<Vec<u8>> {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for &ix in screen {
        let c = &palette[(ix as usize & 0x3f) * 4..];
        // palette order is R,G,B,A → PNG wants R,G,B; A is padding, dropped.
        rgb.extend_from_slice(&[c[0], c[1], c[2]]);
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
