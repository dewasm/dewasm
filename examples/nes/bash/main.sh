#!/usr/bin/env bash
# Interactive terminal frontend for the dewasm-generated NES library
# (nes_gen.sh, produced from examples/apps/cache/nes.wasm by build.sh --
# our nes_demo.c wrapping the agnes emulator). Renders into any
# ANSI truecolor terminal with half-block characters, the same trick as the
# DOOM Bash frontend (../../doom/bash) and the other NES frontends.
#
# The whole point of this frontend, like DOOM's, is that it needs no pixel
# window: the bash backend interprets a full NES frame's worth of emulation
# (CPU + PPU) per tickGame call with no JIT, so this is an existence-proof
# demo -- "it runs at all" -- not a game anyone will play at speed. See
# README.md for the measured, honest framing.
#
# Run with --smoke for a headless self-check (no tty needed): it loads the
# ROM, inits the game, ticks it a few frames, renders one frame to a string
# (verified, never drawn), sanity-checks it, and writes screenshot.ppm.
#
# nes_mem (the module's linear memory) is a global associative array,
# declared by nes_init; this script never uses `set -u` because sparse reads
# of it default to 0, matching the rest of the bash backend -- and never
# leans on `set -e` around a nes_* call either, because a generated function
# routinely computes an intermediate value of exactly 0, which bash treats as
# a "failed" command. The backend's status-cascade convention has every
# generated function return its real status explicitly, so this script checks
# *that* after each call.
set -o pipefail

if (( BASH_VERSINFO[0] < 5 )); then
  echo "nes (bash): requires bash >= 5 (associative arrays, EPOCHREALTIME); found ${BASH_VERSION}. On macOS /bin/bash is 3.2 -- install a newer one (e.g. \`brew install bash\`) and run this script with it." >&2
  exit 1
fi

cd "$(dirname "${BASH_SOURCE[0]}")" || exit 1

readonly ESC=$'\e'
readonly DEFAULT_ROM=../../apps/cache/alter_ego.nes

# Fixed status-line colors (white on black), independent of the game's own
# palette -- without an explicit color the status line inherits whatever
# fg/bg the last-drawn pixel cell left active, flickering with the game.
readonly STATUS_SGR="${ESC}[48;2;0;0;0m${ESC}[38;2;255;255;255m"

# NES controller button bits, matching src/nes_demo.c's setInput().
readonly BTN_A=1 BTN_B=2 BTN_SELECT=4 BTN_START=8
readonly BTN_UP=16 BTN_DOWN=32 BTN_LEFT=64 BTN_RIGHT=128

MODE=interactive
ROM_PATH=$DEFAULT_ROM
if [[ ${1-} == --smoke ]]; then
  MODE=smoke
  [[ -n ${2-} ]] && ROM_PATH=$2
else
  [[ -n ${1-} ]] && ROM_PATH=$1
fi

FRAME_W=0
FRAME_H=0
# Guest pointer to the palette-index screen buffer (one byte per pixel), plus
# the module's fixed 64-entry palette decoded into ready-made SGR escapes and
# R/G/B components. All of it is read once in load_rom: the offsets are stable
# for the emulator's lifetime and the palette never changes, so per-frame work
# is one nes_mem read and one array lookup per sampled pixel.
SCREEN_OFF=0
declare -a FG_SGR BG_SGR PAL_R PAL_G PAL_B

# Overridden by run_interactive once the terminal is in raw/alternate-screen
# mode; a global no-op otherwise so the die paths can call it unconditionally.
restore_terminal() { :; }

# ms-resolution monotonic-ish timestamp for phase/tick timing; no subshell,
# unlike `date`. `10#` forces base-10 so a leading-zero microseconds field
# isn't parsed as an (invalid) octal literal.
epoch_ms() {
  local t=$EPOCHREALTIME
  local sec=${t%.*} frac=${t#*.}
  EPOCH_MS_OUT=$(( sec * 1000 + 10#${frac:0:3} ))
}

fmt_secs() {
  local ms=$1
  printf '%d.%03d' $(( ms / 1000 )) $(( ms % 1000 ))
}

# Calls a nes_* export and exits (after restoring the terminal, a no-op
# outside interactive mode) on anything but success. TRAP_MSG is the only
# place the actual reason lives (the status-cascade convention).
invoke_or_die() {
  local name=$1
  shift
  local status
  nes_invoke "$name" "$@"
  status=$?
  if (( status != 0 )); then
    restore_terminal
    echo "nes (bash): $name failed (status $status): ${TRAP_MSG-unknown error}" >&2
    exit 1
  fi
}

# --- ROM loading -----------------------------------------------------
#
# Unlike DOOM (whose WAD is embedded in its wasm module and delivered through
# a host import), the NES module has zero imports: the host allocates a
# buffer with allocRom(size), copies the iNES ROM bytes straight into the
# module's linear memory (nes_mem) at the returned guest pointer, then calls
# initGame(), which hands that buffer to agnes. `od -An -v -tu1` decodes the
# file to one unsigned decimal byte per token in a single pass -- a ~42KB ROM
# is ~42K array assignments, a few seconds once at startup, irrelevant next
# to the per-frame emulation cost below.
load_rom() {
  local path=$1
  if [[ ! -r $path ]]; then
    echo "nes (bash): ROM not readable: $path" >&2
    exit 1
  fi
  local size
  size=$(wc -c < "$path" | tr -d ' ')
  if (( size <= 0 )); then
    echo "nes (bash): ROM is empty: $path" >&2
    exit 1
  fi

  invoke_or_die _initialize
  invoke_or_die allocRom "$size"
  local ptr=$R0
  if (( ptr == 0 )); then
    echo "nes (bash): allocRom($size) returned a null pointer" >&2
    exit 1
  fi

  local -a bytes
  mapfile -t bytes < <(od -An -v -tu1 "$path" | tr -s ' ' '\n' | grep -v '^$')
  if (( ${#bytes[@]} != size )); then
    echo "nes (bash): decoded ${#bytes[@]} bytes but ROM is $size" >&2
    exit 1
  fi
  local i
  # nes_mem is associative, so the subscript is NOT an arithmetic
  # context and must be pre-evaluated — SC2321's "remove the $((" assumes an
  # indexed array and would store under the literal key "ptr + i".
  # shellcheck disable=SC2321
  for (( i = 0; i < size; i++ )); do
    nes_mem[$(( ptr + i ))]=${bytes[i]}
  done

  invoke_or_die initGame
  if (( R0 != 1 )); then
    echo "nes (bash): initGame() returned $R0 (expected 1) -- ROM not accepted by agnes: $path" >&2
    exit 1
  fi

  invoke_or_die frameWidth;  FRAME_W=$R0
  invoke_or_die frameHeight; FRAME_H=$R0
  invoke_or_die screenOffset; SCREEN_OFF=$R0
  invoke_or_die paletteOffset
  local poff=$R0 e c
  for (( e = 0; e < 64; e++ )); do
    # Palette entries are R,G,B,A (alpha is padding, ignored).
    c=$(( poff + e * 4 ))
    PAL_R[e]=${nes_mem[$c]-0}
    PAL_G[e]=${nes_mem[$(( c + 1 ))]-0}
    PAL_B[e]=${nes_mem[$(( c + 2 ))]-0}
    FG_SGR[e]="${ESC}[38;2;${PAL_R[e]};${PAL_G[e]};${PAL_B[e]}m"
    BG_SGR[e]="${ESC}[48;2;${PAL_R[e]};${PAL_G[e]};${PAL_B[e]}m"
  done
}

# --- Rendering -------------------------------------------------------
#
# Fits a render grid to a terminal (or a synthetic size for --smoke): one
# character cell shows two vertically-stacked source pixels, so cell rows are
# half of sampled logical-pixel rows. Capped at w/2 columns -- for the NES's
# native 256-wide frame that is 128 columns, already wider than most
# terminals people actually run this in, and the same shape as DOOM's cap.
compute_grid() {
  local term_rows=$1 term_cols=$2 w=$3 h=$4
  local avail_rows=$(( term_rows - 1 )) # one row reserved for the status line
  (( avail_rows < 1 )) && avail_rows=1
  local cap_cols=$(( w / 2 ))
  local pixel_cols=$term_cols
  (( pixel_cols > cap_cols )) && pixel_cols=$cap_cols
  (( pixel_cols < 1 )) && pixel_cols=1
  local pixel_rows=$(( (pixel_cols * h + w / 2) / w ))
  local cell_rows=$(( pixel_rows / 2 ))
  (( cell_rows < 1 )) && cell_rows=1
  if (( cell_rows > avail_rows )); then
    cell_rows=$avail_rows
    pixel_rows=$(( cell_rows * 2 ))
    local alt_cols=$(( (pixel_rows * w + h / 2) / h ))
    (( alt_cols < pixel_cols )) && pixel_cols=$alt_cols
  fi
  GRID_COLS=$pixel_cols
  GRID_ROWS=$cell_rows
  PIXEL_ROWS=$pixel_rows
}

# Builds one frame's worth of ANSI half-block cells into RENDER_OUT.
# Deliberately flat (no helper calls, no command substitution, no per-cell
# printf) inside the nested loops -- string concatenation and arithmetic
# only. An SGR code is skipped when it repeats the previous cell's, which is
# cheap and shrinks the string a lot on the NES's large flat-color areas (its
# PPU has a fixed 64-entry palette, <=25 colors on screen at once).
render_frame() {
  local buf="" cy cx top_base bot_base src_x ti bi
  local last_fg=-1 last_bg=-1
  local w=$FRAME_W h=$FRAME_H off=$SCREEN_OFF
  for (( cy = 0; cy < GRID_ROWS; cy++ )); do
    buf+="${ESC}[$(( cy + 1 ));1H"
    top_base=$(( ((cy * 2) * h / PIXEL_ROWS) * w ))
    bot_base=$(( ((cy * 2 + 1) * h / PIXEL_ROWS) * w ))
    for (( cx = 0; cx < GRID_COLS; cx++ )); do
      src_x=$(( cx * w / GRID_COLS ))
      # One byte per pixel, a palette index; the & 0x3f mask is load-bearing.
      # Associative-array subscripts are literal strings, not arithmetic
      # expressions (unlike inside `$(( ))`), so every subscript needs an
      # explicit `$`/`$(( ))` -- a bare `nes_mem[o]` would look up the key
      # "o", not the address.
      ti=$(( ${nes_mem[$(( off + top_base + src_x ))]-0} & 0x3f ))
      bi=$(( ${nes_mem[$(( off + bot_base + src_x ))]-0} & 0x3f ))
      if (( ti != last_fg )); then
        buf+=${FG_SGR[ti]}
        last_fg=$ti
      fi
      if (( bi != last_bg )); then
        buf+=${BG_SGR[bi]}
        last_bg=$bi
      fi
      buf+="▀"
    done
  done
  printf -v RENDER_OUT '%s' "$buf"
}

# Dumps the same sampled grid render_frame uses as an ASCII PPM (P3): text,
# so there's no risk of a stray NUL confusing anything downstream.
write_ppm() {
  local path=$1
  local w=$FRAME_W h=$FRAME_H off=$SCREEN_OFF
  local py px row_base src_x ix r g b
  local buf; buf=$'P3\n'"${GRID_COLS} ${PIXEL_ROWS}"$'\n255\n'
  declare -A distinct=()
  for (( py = 0; py < PIXEL_ROWS; py++ )); do
    row_base=$(( (py * h / PIXEL_ROWS) * w ))
    for (( px = 0; px < GRID_COLS; px++ )); do
      src_x=$(( px * w / GRID_COLS ))
      ix=$(( ${nes_mem[$(( off + row_base + src_x ))]-0} & 0x3f ))
      r=${PAL_R[ix]}; g=${PAL_G[ix]}; b=${PAL_B[ix]}
      buf+="$r $g $b"$'\n'
      distinct["$r,$g,$b"]=1
    done
  done
  printf '%s' "$buf" > "$path"
  DISTINCT_COLORS=${#distinct[@]}
}

# --- Input (interactive mode only) -----------------------------------
#
# Terminals deliver key *presses* only, never releases. There is no
# meaningful "held key" at this frame rate, so a press is held for exactly
# the tick it was seen before: setInput() gets the bitmask of everything seen
# since the previous tick just before tickGame, then the next frame's drain
# starts from an empty set again (setInput(0) unless re-pressed) -- one tick
# of "held down," as fine-grained as input can get here.
DOWN_MASK=0
QUIT=0

# Key map (see README): arrows = D-pad, x = A, z = B, Enter = Start,
# Space = Select, q / Ctrl-C = quit.
handle_char() {
  local c=$1
  case "$c" in
    $'\x03' | q | Q) QUIT=1 ;;
    x | X) (( DOWN_MASK |= BTN_A )) ;;
    z | Z) (( DOWN_MASK |= BTN_B )) ;;
    $'\x0d') (( DOWN_MASK |= BTN_START )) ;;   # Enter
    ' ') (( DOWN_MASK |= BTN_SELECT )) ;;      # Space
    *) ;;
  esac
}

# Drains whatever bytes queued on stdin since the last call and folds them
# into DOWN_MASK. Arrow keys arrive as ESC [ A/B/C/D; a lone ESC is dropped
# (the NES has no Escape). Reads are raw single bytes with a tiny timeout so
# the poll is non-blocking, matching DOOM's drain_input.
drain_input() {
  DOWN_MASK=0
  local ch c2 c3
  while IFS= read -rs -t 0.01 -n 1 ch; do
    if [[ $ch == "$ESC" ]]; then
      if IFS= read -rs -t 0.01 -n 1 c2 && [[ $c2 == '[' ]]; then
        if IFS= read -rs -t 0.01 -n 1 c3; then
          case "$c3" in
            A) (( DOWN_MASK |= BTN_UP )) ;;
            B) (( DOWN_MASK |= BTN_DOWN )) ;;
            C) (( DOWN_MASK |= BTN_RIGHT )) ;;
            D) (( DOWN_MASK |= BTN_LEFT )) ;;
            *) : ;; # an unrecognized CSI final byte (e.g. an F-key): drop it
          esac
        fi
      elif [[ -n $c2 ]]; then
        # Not a CSI sequence: the ESC was spurious, c2 is a fresh byte.
        handle_char "$c2"
      fi
    else
      handle_char "$ch"
    fi
  done
}

# --- Modes -----------------------------------------------------------

run_smoke() {
  epoch_ms; local t0=$EPOCH_MS_OUT
  nes_init
  local status=$?
  (( status != 0 )) && { echo "smoke: FAIL: nes_init (status $status): ${TRAP_MSG-unknown error}" >&2; exit 1; }
  epoch_ms; local t1=$EPOCH_MS_OUT
  echo "smoke: nes_init (memory + data/elem segments) in $(fmt_secs $((t1 - t0)))s"

  echo "smoke: loading ROM $ROM_PATH (_initialize + allocRom + byte copy + initGame)..."
  epoch_ms; t0=$EPOCH_MS_OUT
  load_rom "$ROM_PATH"
  epoch_ms; t1=$EPOCH_MS_OUT
  echo "smoke: ROM loaded, initGame OK, ${FRAME_W}x${FRAME_H} framebuffer, in $(fmt_secs $((t1 - t0)))s"

  # Tick to SMOKE_FRAMES with no input, the same driving contract as the
  # cross-backend framebuffer snapshot (crates/dewasm-test-helper/src/nes.rs):
  # Alter Ego opens on a near-black boot frame (1 color at ~15
  # ticks) and only fades in its final, stable credits screen (7 distinct
  # colors) by frame ~37, so a shorter run would only ever screenshot black.
  # At tens of seconds per frame this is ~20 minutes -- a progress line per
  # tick keeps the silence from ever looking like a hang. Set SMOKE_FRAMES=N
  # for a shorter
  # pipeline check (fewer than ~37 frames will legitimately be near-black).
  local frames=${SMOKE_FRAMES:-40}
  local total_ms=0 i
  for (( i = 1; i <= frames; i++ )); do
    epoch_ms; t0=$EPOCH_MS_OUT
    invoke_or_die setInput 0
    invoke_or_die tickGame
    epoch_ms; t1=$EPOCH_MS_OUT
    total_ms=$(( total_ms + (t1 - t0) ))
    echo "smoke: tick $i/$frames in $(fmt_secs $((t1 - t0)))s"
  done
  (( frames > 0 )) && echo "smoke: mean $(fmt_secs $(( total_ms / frames )))s/frame over $frames frames"

  # A synthetic terminal matching the cap this frontend applies.
  compute_grid 61 128 "$FRAME_W" "$FRAME_H"

  epoch_ms; t0=$EPOCH_MS_OUT
  render_frame
  epoch_ms; t1=$EPOCH_MS_OUT
  echo "smoke: rendered one ${GRID_COLS}x${GRID_ROWS}-cell frame (sampled from a ${GRID_COLS}x${PIXEL_ROWS} logical-pixel grid, not the full ${FRAME_W}x${FRAME_H} framebuffer) in $(fmt_secs $((t1 - t0)))s -- to a string only, never drawn"

  if [[ $RENDER_OUT != *"▀"* ]]; then
    echo "smoke: FAIL: rendered frame has no half-block glyphs" >&2
    exit 1
  fi
  if [[ $RENDER_OUT != *"${ESC}[38;2;"* ]]; then
    echo "smoke: FAIL: rendered frame has no truecolor SGR sequences" >&2
    exit 1
  fi
  echo "smoke: rendered frame contains half-block glyphs and truecolor SGR sequences"

  write_ppm screenshot.ppm
  echo "smoke: wrote $(pwd)/screenshot.ppm (${GRID_COLS}x${PIXEL_ROWS}, $DISTINCT_COLORS distinct colors)"
  # The NES PPU renders from a fixed 64-color hardware palette, so a healthy
  # frame lands in the low dozens at most; a degenerate (blank/solid) frame
  # lands in the single digits.
  if (( DISTINCT_COLORS <= 2 )); then
    echo "smoke: FAIL: frame looks degenerate (too few distinct colors)" >&2
    exit 1
  fi

  echo "smoke: PASS"
}

run_interactive() {
  if [[ ! -t 0 || ! -t 1 ]]; then
    echo "nes (bash): interactive mode needs a real terminal on stdin/stdout (try \`./main.sh --smoke\` for a headless check)" >&2
    exit 1
  fi

  nes_init
  local status=$?
  if (( status != 0 )); then
    echo "nes (bash): nes_init failed (status $status): ${TRAP_MSG-unknown error}" >&2
    exit 1
  fi

  # Enter the alternate screen before ROM load/initGame even start (as DOOM
  # does): boot alone is not instant, and Ctrl-C/kill during it then
  # exercises the same restore path as quitting from the game loop.
  ORIG_STTY=$(stty -g)
  restore_terminal() {
    stty "$ORIG_STTY" 2>/dev/null || true
    # SGR reset first: the fixed status-line colors otherwise persist past
    # leaving the alternate screen and tint the shell prompt underneath.
    printf '%s' "${ESC}[0m${ESC}[?25h${ESC}[?1049l"
  }
  # Ctrl-C is handled as a byte in drain_input (raw mode disables the
  # terminal's own SIGINT); the INT/TERM traps are a backstop for `kill` or a
  # signal during boot. A custom INT/TERM trap does not itself end the
  # process, so these must call exit explicitly (EXIT then also fires;
  # restore_terminal is idempotent, so running it twice is harmless).
  trap restore_terminal EXIT
  trap 'restore_terminal; exit 130' INT
  trap 'restore_terminal; exit 143' TERM
  stty raw -echo
  printf '%s' "${ESC}[?1049h${ESC}[?25l${ESC}[2J${ESC}[H"
  # -echo means a plain printf won't return to column 1 on its own; \r\n
  # (not bare \n) keeps boot progress readable.
  boot_msg() { printf '%s\r\n' "$1"; }

  boot_msg "dewasm NES (bash) -- loading ROM $ROM_PATH and booting agnes; this is not instant."
  load_rom "$ROM_PATH"
  boot_msg "initGame done, ${FRAME_W}x${FRAME_H} framebuffer. Each frame is emulated by bash -- expect it slow. Press q to quit."

  local term_rows term_cols
  read -r term_rows term_cols < <(stty size 2>/dev/null) || { term_rows=${LINES:-24}; term_cols=${COLUMNS:-80}; }
  compute_grid "$term_rows" "$term_cols" "$FRAME_W" "$FRAME_H"
  printf '%s' "${ESC}[2J${ESC}[H" # clear the boot-progress scrollback

  local frame=0 dt_ms=0
  while (( QUIT == 0 )); do
    drain_input
    (( QUIT )) && break

    invoke_or_die setInput "$DOWN_MASK"

    epoch_ms; local t0=$EPOCH_MS_OUT
    invoke_or_die tickGame
    epoch_ms; local t1=$EPOCH_MS_OUT
    dt_ms=$(( t1 - t0 ))

    (( frame++ ))
    render_frame
    local status_text
    status_text="dewasm NES (bash) | frame $frame | $(fmt_secs "$dt_ms")s/frame | q quit  arrows d-pad  x A  z B  enter Start  space Select"
    printf '%s' "$RENDER_OUT${ESC}[$(( GRID_ROWS + 1 ));1H${ESC}[0m${STATUS_SGR}${ESC}[K${status_text}"
  done
}

# The generated NES library.
# shellcheck source=/dev/null
source ./nes_gen.sh

if [[ $MODE == smoke ]]; then
  run_smoke
else
  run_interactive
fi
