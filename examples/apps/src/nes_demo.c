/*
 * nes_demo.c: our own committed C source.
 * First-party source is fine to commit; only third-party *artifacts* stay out of the tree.
 *
 * A thin reactor wrapper around `agnes` (`kgabis/agnes`, version fixed in `scripts/nes.sh`).
 * It exposes a minimal driving surface with no imports for the NES frame snapshot example.
 * That example is issue #114, and it mirrors the shape of the DOOM example.
 * A host allocates the ROM and loads it.
 * It then ticks frames with an input bit mask, and reads the frame straight out of guest memory.
 *
 * The frame is handed over exactly as `agnes` keeps it.
 * That is a 256x240 buffer of one palette *index* per pixel, plus the fixed 64-entry palette.
 * The other choice was a BGRA image rendered in the guest per pixel.
 * That cost 12-15% of frame time on every backend for no added information (issue #117).
 * A host composes a pixel as `palette[screen[y * width + x] & 0x3f]` → R,G,B.
 * The 4th palette byte is alpha padding, and a host can ignore it.
 * **The `& 0x3f` mask is required**: indices above 63 do occur in the buffer.
 * The accessor of `agnes` itself masks them; a host that skips the mask reads past the palette.
 *
 * `agnes.c` is a single-file amalgamation whose type and object definitions live at file scope.
 * So `#include`-ing it here makes `ppu.screen_buffer` and `g_colors` reachable.
 * Compiling it as a separate translation unit instead would need a patch to third-party source.
 *
 * Having no imports is a goal: we avoid `stdio`/`assert` so `wasi-libc` pulls nothing in.
 * That leaves the import section of the module empty (verified with `wasm-dis` in `nes.sh`).
 * Only `malloc`/`memset` (no imports) and `agnes` itself are used.
 */
#include <stdlib.h>
#include <string.h>

#include "agnes.c"

/* The loaded ROM buffer, remembered between `allocRom` and `initGame`. */
static void *g_rom = NULL;
static int g_rom_size = 0;
static agnes_t *g_agnes = NULL;

/*
 * Allocate a `size`-byte buffer for the iNES ROM, remember it, and return its guest pointer.
 * The host then copies the ROM bytes in before `initGame`.
 */
int allocRom(int size) {
  g_rom = malloc((size_t)size);
  g_rom_size = size;
  return (int)(unsigned long)g_rom;
}

/*
 * Create the emulator and load the ROM previously copied into `g_rom`.
 * Returns 1 on success, 0 on failure (the hosts assert on the result).
 */
int initGame(void) {
  g_agnes = agnes_make();
  if (g_agnes == NULL) {
    return 0;
  }
  return agnes_load_ines_data(g_agnes, g_rom, (size_t)g_rom_size) ? 1 : 0;
}

/*
 * Set the controller of player 1 from a button bit mask.
 * Bit order: A=1, B=2, Select=4, Start=8, Up=16, Down=32, Left=64, Right=128.
 * Player 2 is left unpressed.
 */
void setInput(int buttons) {
  agnes_input_t in;
  in.a = (buttons & 1) != 0;
  in.b = (buttons & 2) != 0;
  in.select = (buttons & 4) != 0;
  in.start = (buttons & 8) != 0;
  in.up = (buttons & 16) != 0;
  in.down = (buttons & 32) != 0;
  in.left = (buttons & 64) != 0;
  in.right = (buttons & 128) != 0;
  agnes_set_input(g_agnes, &in, NULL);
}

/* Emulate one full video frame, leaving it in the screen buffer of `agnes`. */
void tickGame(void) { agnes_next_frame(g_agnes); }

/*
 * Guest pointer to the palette-index screen buffer.
 * It holds `frameWidth * frameHeight` bytes, row-major, one index per pixel.
 * It is valid only after a successful `initGame`, since it points inside the emulator made there.
 * It stays the same for the lifetime of that emulator.
 */
int screenOffset(void) { return (int)(unsigned long)g_agnes->ppu.screen_buffer; }

/*
 * Guest pointer to the palette: 64 entries of 4 bytes each, R,G,B,A (alpha is padding).
 * The data does not change, so reading it once is enough.
 */
int paletteOffset(void) { return (int)(unsigned long)g_colors; }

/* Framebuffer dimensions (`AGNES_SCREEN_WIDTH`/`AGNES_SCREEN_HEIGHT`). */
int frameWidth(void) { return AGNES_SCREEN_WIDTH; }
int frameHeight(void) { return AGNES_SCREEN_HEIGHT; }
