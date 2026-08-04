#!/usr/bin/env python3
"""Interactive frontend for the dewasm-generated NES emulator (nes_gen.py,
produced from examples/apps/src/nes_demo.c's agnes wrapper by build.sh,
ADR-59). Unlike DOOM, nes.wasm has zero wasm imports -- no console/save-
game/clock host surface to wire up, just `_initialize` plus the seven demo
exports (allocRom/initGame/setInput/tickGame/frameOffset/frameWidth/
frameHeight) -- so this script's whole job is to load a ROM into the
module's linear memory and drive the game loop itself: pacing, input
polling, and rendering are entirely the host's job (the module has no clock
import of its own, unlike DOOM's internal 35Hz timer).

Rendering reuses the DOOM Python frontend's half-block truecolor trick
(../../doom/python/) and its key-hold heuristic for terminals that only
deliver key presses; the controller is level-triggered (one
`setInput(bitmask)` call per tick, not DOOM's edge-triggered
reportKeyDown/reportKeyUp pair), so the heuristic here just tracks which
buttons are currently "down" and recomputes the bitmask every tick instead
of invoking anything on press/release.

Run with --smoke for a headless self-check (no terminal takeover): it loads
the ROM, ticks the emulator a fixed number of frames, and writes the last
frame to screenshot.ppm.
"""
import os
import select
import sys
import termios
import time
import tty

import nes_gen

DEFAULT_ROM = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "apps", "cache", "alter_ego.nes")

# Controller bit layout, fixed by nes_demo.c's setInput contract.
BIT_A = 1
BIT_B = 2
BIT_SELECT = 4
BIT_START = 8
BIT_UP = 16
BIT_DOWN = 32
BIT_LEFT = 64
BIT_RIGHT = 128

# A pressed key is held down-active until this many seconds pass without
# seeing it again -- terminals only deliver key-down events (no key-up), so
# releases have to be synthesized. This backend's tick rate is close to
# DOOM's Python frontend's (dewasm's Python backend cost is dominated by
# per-instruction interpretation overhead, not game complexity -- see
# README), so this reuses that frontend's RELEASE_TIMEOUT rather than the
# much shorter one the Ruby/Perl NES frontends use at their much higher tick
# rates: the window has to bridge the gap between ticks, not just a
# terminal's own autorepeat interval.
RELEASE_TIMEOUT = 0.4

FRAME_W = 256
FRAME_H = 240

nes = None
frame_off = 0


def read_rom(path):
    with open(path, "rb") as f:
        return f.read()


def load_rom(path):
    data = read_rom(path)
    ptr = nes.invoke("allocRom", len(data))
    nes.memory.data[ptr : ptr + len(data)] = data
    ok = nes.invoke("initGame")
    assert ok == 1, f"initGame failed (ROM: {path})"


# --- Terminal rendering ------------------------------------------------
#
# Each character cell shows two vertically-stacked pixels via the upper-half
# block character, foreground = top pixel, background = bottom pixel. Unlike
# DOOM's 640x400 framebuffer (a 2x upscale of its native 320x200), agnes's
# 256x240 framebuffer already is the native NES resolution, so pixels are
# read 1:1 and nearest-neighbor sampled down to however many columns/rows
# actually fit.

UPPER_HALF_BLOCK = "▀"


def _pixel(mv, off, buf_w, x, y):
    # Memory byte order is B, G, R, A (see nes_demo.c's tickGame).
    o = off + (y * buf_w + x) * 4
    return mv[o + 2], mv[o + 1], mv[o]


def _fit(logical_w, logical_h, term_cols, term_rows):
    """Returns (width, rows) of terminal cells to render into: width in
    character columns (== pixel columns, 1 pixel per column), rows in
    character rows (== pixel-row-pairs, 2 pixels per row), leaving one line
    for the status line, never upscaling past the logical resolution."""
    avail_rows = max(term_rows - 1, 1)
    width = max(min(term_cols, logical_w), 1)
    height_px = max(width * logical_h // logical_w, 2)
    rows = height_px // 2
    if rows > avail_rows:
        rows = avail_rows
        height_px = rows * 2
        width = max(min(height_px * logical_w // logical_h, term_cols, logical_w), 1)
    return width, rows


def build_cells(mv, off, buf_w, buf_h, width, rows):
    height_px = rows * 2
    cells = [[None] * width for _ in range(rows)]
    for row in range(rows):
        ly_top = row * 2 * buf_h // height_px
        ly_bot = (row * 2 + 1) * buf_h // height_px
        cur = cells[row]
        for col in range(width):
            lx = col * buf_w // width
            cur[col] = (
                _pixel(mv, off, buf_w, lx, ly_top),
                _pixel(mv, off, buf_w, lx, ly_bot),
            )
    return cells


def diff_escapes(cells, prev, rows, width):
    """Builds one escape-code string covering only the cells that changed
    since prev (None means: everything is new, no cells are skipped). Cursor
    moves skip unchanged runs; repeated foreground/background colors within
    a changed run are not re-emitted as SGR codes."""
    out = []
    for row in range(rows):
        cur_row = cells[row]
        prev_row = prev[row] if prev is not None else None
        col = 0
        while col < width:
            if prev_row is not None and cur_row[col] == prev_row[col]:
                col += 1
                continue
            out.append(f"\x1b[{row + 1};{col + 1}H")
            last_fg = last_bg = None
            while col < width and (prev_row is None or cur_row[col] != prev_row[col]):
                fg, bg = cur_row[col]
                if fg != last_fg:
                    out.append(f"\x1b[38;2;{fg[0]};{fg[1]};{fg[2]}m")
                    last_fg = fg
                if bg != last_bg:
                    out.append(f"\x1b[48;2;{bg[0]};{bg[1]};{bg[2]}m")
                    last_bg = bg
                out.append(UPPER_HALF_BLOCK)
                col += 1
        out.append("\x1b[0m")
    return "".join(out)


class Renderer:
    """Keeps the previous frame's cells so render() can emit a diff instead
    of repainting the whole screen every tick."""

    def __init__(self):
        self.prev = None
        self.width = 0
        self.rows = 0

    def render(self, status_line):
        mv = memoryview(nes.memory.data)
        term_cols, term_rows = os.get_terminal_size()
        width, rows = _fit(FRAME_W, FRAME_H, term_cols, term_rows)
        cells = build_cells(mv, frame_off, FRAME_W, FRAME_H, width, rows)

        resized = width != self.width or rows != self.rows
        prev = None if resized else self.prev
        out = "\x1b[2J" if resized else ""
        out += diff_escapes(cells, prev, rows, width)
        # Status line always redrawn: it's one line, and its own text changes tick to tick.
        out += f"\x1b[{rows + 1};1H\x1b[0m\x1b[K{status_line}"
        os.write(1, out.encode())

        self.prev, self.width, self.rows = cells, width, rows


# --- Input ---------------------------------------------------------------
#
# Terminals deliver key presses only, in raw mode as bytes on stdin: arrow
# keys as 3-byte escape sequences, everything else as 1 byte. A lone ESC
# keypress is indistinguishable from the first byte of an escape sequence
# until either more bytes show up (they arrive together, already buffered,
# for a real escape sequence) or a short timeout passes with nothing more
# arriving (a real ESC keypress).
_ESC_TIMEOUT = 0.01

_ARROW_BITS = {b"A": BIT_UP, b"B": BIT_DOWN, b"C": BIT_RIGHT, b"D": BIT_LEFT}


def read_key(fd):
    """Reads and decodes one key event from fd. Returns a controller bit,
    "QUIT", or None for anything unrecognized/unmapped."""
    b = os.read(fd, 1)
    if not b:
        return None
    if b == b"\x1b":
        if select.select([fd], [], [], _ESC_TIMEOUT)[0]:
            b2 = os.read(fd, 1)
            if b2 == b"[" and select.select([fd], [], [], _ESC_TIMEOUT)[0]:
                b3 = os.read(fd, 1)
                return _ARROW_BITS.get(b3)
        return None
    if b in (b"\x03", b"q"):
        return "QUIT"
    if b in (b"x", b"X"):
        return BIT_A
    if b in (b"z", b"Z"):
        return BIT_B
    if b == b"\r":
        return BIT_START
    if b == b" ":
        return BIT_SELECT
    return None


# The NTSC NES runs at ~60.0988Hz; 60 exactly is close enough that no
# separate calibration is needed (unlike DOOM's internal 35Hz pacing, which
# the host has no say over at all -- here pacing is entirely the host's job).
TARGET_FPS = 60
FRAME_SECONDS = 1.0 / TARGET_FPS


def run_interactive(rom_path):
    global nes, frame_off
    nes = nes_gen.Nes({})
    nes.invoke("_initialize")
    load_rom(rom_path)
    frame_off = nes.invoke("frameOffset")

    fd = sys.stdin.fileno()
    old_settings = termios.tcgetattr(fd)

    def restore():
        termios.tcsetattr(fd, termios.TCSADRAIN, old_settings)
        os.write(1, b"\x1b[?25h\x1b[?1049l")

    renderer = Renderer()
    held = {}  # controller bit -> release deadline (time.monotonic() seconds)
    status_ticks = 0
    status_window_start = time.monotonic()
    status = "dewasm NES (Python) -- starting... -- q: quit"

    tty.setraw(fd)
    os.write(1, b"\x1b[?1049h\x1b[?25l")
    try:
        # Fixed-timestep pacing: sleep when running ahead of schedule; when
        # the interpreter can't keep up, never sleep and just resync the
        # schedule to "now" instead of trying to burn through a backlog of
        # missed frames.
        next_frame_at = time.monotonic()
        while True:
            events = []
            while select.select([fd], [], [], 0)[0]:
                ev = read_key(fd)
                if ev is not None:
                    events.append(ev)

            now = time.monotonic()
            quit_requested = False
            for ev in events:
                if ev == "QUIT":
                    quit_requested = True
                    continue
                held[ev] = now + RELEASE_TIMEOUT  # (re-)arm/extend the release deadline

            for bit, deadline in list(held.items()):
                if now >= deadline:
                    del held[bit]

            if quit_requested:
                break

            mask = 0
            for bit in held:
                mask |= bit
            nes.invoke("setInput", mask)
            nes.invoke("tickGame")
            frame_off = nes.invoke("frameOffset")

            status_ticks += 1
            elapsed = now - status_window_start
            if elapsed >= 0.5:
                rate = status_ticks / elapsed
                status = f"dewasm NES (Python) -- {rate:.2f} frames/sec -- q: quit"
                status_ticks = 0
                status_window_start = now
            renderer.render(status)

            next_frame_at += FRAME_SECONDS
            now2 = time.monotonic()
            if next_frame_at > now2:
                time.sleep(next_frame_at - now2)
            else:
                next_frame_at = now2
    finally:
        restore()


# --- Headless self-check ---------------------------------------------------


def write_ppm_and_count_colors(path, mv, off, w, h):
    """Writes the framebuffer as a binary P6 PPM (stdlib-only, unlike PNG)
    and counts distinct RGB colors in the same pass."""
    distinct = set()
    buf = bytearray(w * h * 3)
    for i in range(w * h):
        o = off + i * 4
        b, g, r = mv[o], mv[o + 1], mv[o + 2]
        j = i * 3
        buf[j], buf[j + 1], buf[j + 2] = r, g, b
        distinct.add((r, g, b))
    with open(path, "wb") as f:
        f.write(f"P6\n{w} {h}\n255\n".encode("ascii"))
        f.write(buf)
    return distinct


# Matches NES_FRAMES in crates/dewasm-test-helper/src/nes.rs (ADR-59): the
# smallest input-free tick count that clears Alter Ego's boot into its
# stable credits screen, which also lets a smoke run's screenshot.ppm be
# diffed directly against examples/apps/snapshots/nes_frame.ppm. Perl (the
# other frontend slow enough to feel every extra tick) uses the same count;
# Ruby/Go/Java run several hundred instead since their tick cost is
# negligible.
SMOKE_FRAMES = 40


def run_smoke(rom_path, frames=SMOKE_FRAMES):
    global nes, frame_off
    nes = nes_gen.Nes({})
    nes.invoke("_initialize")
    load_rom(rom_path)

    start = time.monotonic()
    for _ in range(frames):
        nes.invoke("setInput", 0)
        nes.invoke("tickGame")
    elapsed = time.monotonic() - start
    frame_off = nes.invoke("frameOffset")
    rate = frames / elapsed if elapsed > 0 else float("inf")
    print(f"smoke: ran {frames} frames in {elapsed:.3f}s ({rate:.2f} frames/sec)")

    mv = memoryview(nes.memory.data)

    # Measure render cost without a real terminal: build the escape string
    # for a representative fixed size and throw it away instead of writing
    # it to a screen.
    render_start = time.monotonic()
    cols, rows = 80, 24
    width, rows = _fit(FRAME_W, FRAME_H, cols, rows)
    cells = build_cells(mv, frame_off, FRAME_W, FRAME_H, width, rows)
    diff_escapes(cells, None, rows, width)
    render_elapsed = time.monotonic() - render_start
    print(f"smoke: terminal-render (to a string) took {render_elapsed * 1000:.2f}ms for a {width}x{rows}-cell frame")

    distinct = write_ppm_and_count_colors("screenshot.ppm", mv, frame_off, FRAME_W, FRAME_H)
    print(f"smoke: final frame is {FRAME_W}x{FRAME_H}, wrote screenshot.ppm ({len(distinct)} distinct colors)")

    # The NES PPU palette tops out at 64 colors, and agnes's frame is a
    # small, mostly-flat subset of it (Alter Ego's title screen settles at 7
    # -- see ADR-59 and NES_FRAMES's comment in
    # crates/dewasm-test-helper/src/nes.rs), so a healthy frame is nowhere
    # near the thousands of colors a truecolor renderer would produce; a
    # degenerate (blank/solid) frame is the real signal to catch, and lands
    # in the single digits. Mirrors the >4 threshold the snapshot oracle
    # uses (crates/xtask/src/nes_snapshot.rs) and the Ruby/Perl frontends.
    ok = True
    if len(distinct) <= 4:
        print("smoke: FAIL: frame looks degenerate (too few distinct colors)", file=sys.stderr)
        ok = False

    if not ok:
        sys.exit(1)
    print("smoke: OK")


def main():
    args = sys.argv[1:]
    smoke = "--smoke" in args
    args = [a for a in args if a != "--smoke"]
    rom_path = args[0] if args else DEFAULT_ROM

    if smoke:
        run_smoke(rom_path)
    else:
        run_interactive(rom_path)


if __name__ == "__main__":
    main()
