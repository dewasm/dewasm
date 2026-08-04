/*
 * nes_demo.c — our own committed C source (ADR-9 permits committing first-party
 * source; only third-party *artifacts* stay out of the tree).
 *
 * A thin reactor wrapper around agnes (kgabis/agnes, pinned by
 * examples/apps/scripts/nes.sh) that exposes a minimal, import-free driving
 * surface for the NES framebuffer-snapshot example (issue #114), mirroring the
 * DOOM demo's shape (ADR-50/53): a host allocates the ROM, loads it, ticks
 * frames with an input bitmask, then reads a B,G,R,A framebuffer straight out of
 * guest memory. The framebuffer byte order matches doom.wasm's, so the
 * test-helper's frame_to_ppm and the per-backend glue are reused unchanged.
 *
 * Import-free is a goal: we avoid stdio/assert so wasi-libc pulls nothing in,
 * leaving the module's import section empty (verified with wasm-dis in
 * nes.sh). Only malloc/memset (no imports) and agnes itself are used.
 */
#include <stdlib.h>
#include <string.h>

#include "agnes.h"

/* The loaded ROM buffer, remembered between allocRom and initGame. */
static void *g_rom = NULL;
static int g_rom_size = 0;
static agnes_t *g_agnes = NULL;

/* The rendered frame, B,G,R,A per pixel (alpha padding), matching doom.wasm's
 * layout so the shared PPM encoder and glue apply as-is. */
static unsigned char g_frame[AGNES_SCREEN_WIDTH * AGNES_SCREEN_HEIGHT * 4];

/* Allocate a size-byte buffer for the iNES ROM, remember it, and return its
 * guest pointer so the host can copy the ROM bytes in before initGame. */
int allocRom(int size) {
  g_rom = malloc((size_t)size);
  g_rom_size = size;
  return (int)(unsigned long)g_rom;
}

/* Create the emulator and load the ROM previously copied into g_rom. Returns 1
 * on success, 0 on failure (the hosts assert on the result). */
int initGame(void) {
  g_agnes = agnes_make();
  if (g_agnes == NULL) {
    return 0;
  }
  return agnes_load_ines_data(g_agnes, g_rom, (size_t)g_rom_size) ? 1 : 0;
}

/* Set player 1's controller from a button bitmask. Bit order:
 *   A=1, B=2, Select=4, Start=8, Up=16, Down=32, Left=64, Right=128.
 * Player 2 is left unpressed. */
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

/* Emulate one full video frame, then snapshot every pixel into g_frame in
 * B,G,R,A order (agnes reports R,G,B,A; A is padding, kept as 0xff). */
void tickGame(void) {
  agnes_next_frame(g_agnes);
  int i = 0;
  for (int y = 0; y < AGNES_SCREEN_HEIGHT; y++) {
    for (int x = 0; x < AGNES_SCREEN_WIDTH; x++) {
      agnes_color_t c = agnes_get_screen_pixel(g_agnes, x, y);
      g_frame[i++] = c.b;
      g_frame[i++] = c.g;
      g_frame[i++] = c.r;
      g_frame[i++] = 0xff;
    }
  }
}

/* Guest pointer to the rendered framebuffer. */
int frameOffset(void) { return (int)(unsigned long)g_frame; }

/* Framebuffer dimensions (AGNES_SCREEN_WIDTH/AGNES_SCREEN_HEIGHT). */
int frameWidth(void) { return AGNES_SCREEN_WIDTH; }
int frameHeight(void) { return AGNES_SCREEN_HEIGHT; }
