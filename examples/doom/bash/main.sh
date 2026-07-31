#!/usr/bin/env bash
# Interactive terminal frontend for the dewasm-generated DOOM library
# (doom_gen.sh, produced from jacobenget/doom.wasm by build.sh). Renders
# into any ANSI truecolor terminal with half-block characters, same trick
# as ../ruby and ../python -- there is no pixel-window option here because
# the point of this frontend is that it needs none: the bash backend runs
# DOOM's initGame in ~103s and each tickGame in ~34s (measured on the
# reference machine, after the ADR-51 associative-memory and ADR-52
# inline-load/store work; see ../README.md and docs/adr/51-*.md,
# docs/adr/52-*.md), so this is an existence-proof demo -- about two
# minutes to boot, one frame roughly every half a minute -- not a game
# anyone will speedrun. See README.md for the honest framing.
#
# Run with --smoke for a headless self-check (no tty/alternate-screen
# needed): it inits the game, ticks it twice, renders one frame to a string
# (verified, never drawn), sanity-checks it, and writes screenshot.ppm.
#
# doom_mem (the module's linear memory) is a global associative array,
# declared by doom_init; this script never uses `set -u` because sparse
# reads of it are meant to default to 0, matching the rest of the bash
# backend (see docs/adr/51-bash-assoc-memory.md) -- and never relies on
# `set -e` around any doom_* call either, because a generated function's
# internal arithmetic routinely computes an intermediate value of exactly
# 0, which bash treats as a "failed" command; the backend's own
# status-cascade convention (docs/adr/11-bash-backend-lowering.md) already
# has every generated function return its real status explicitly via
# `return 0`/`return $?`, so this script checks *that* after every call
# instead of leaning on errexit.
set -o pipefail

if (( BASH_VERSINFO[0] < 5 )); then
  echo "doom (bash): requires bash >= 5 (associative arrays, EPOCHREALTIME); found ${BASH_VERSION}. On macOS /bin/bash is 3.2 -- install a newer one (e.g. \`brew install bash\`) and run this script with it." >&2
  exit 1
fi

cd "$(dirname "${BASH_SOURCE[0]}")" || exit 1

readonly ESC=$'\e'

MODE=interactive
[[ ${1-} == --smoke ]] && MODE=smoke
# Info messages would corrupt the alternate-screen frame if printed there,
# so they're dropped entirely in interactive mode; --smoke has no
# alternate screen, so they're shown (matching ../ruby, ../python).
SUPPRESS_INFO=1
[[ $MODE == smoke ]] && SUPPRESS_INFO=0

FRAME_W=0
FRAME_H=0
FRAME_BUF_OFF=0

# Overridden by run_interactive once the terminal is actually in raw/
# alternate-screen mode; a global no-op otherwise so invoke_or_die/the
# doom_init failure path can call it unconditionally in either mode.
restore_terminal() { :; }

# Decodes a UTF-8 byte range out of doom_mem into MEM_TEXT_OUT. Only used
# for the handful of console messages emitted around boot, so a per-byte
# loop is not a hot path (unlike the framebuffer, which is orders of
# magnitude larger and read directly by render_frame/write_ppm instead).
mem_to_text() {
  local off=$1 len=$2 i b hex out=""
  for (( i = 0; i < len; i++ )); do
    b=${doom_mem[$((off + i))]-0}
    printf -v hex '\\x%02x' "$b"
    out+=$hex
  done
  printf -v MEM_TEXT_OUT '%b' "$out"
}

imp_on_error() {
  mem_to_text "$1" "$2"
  printf '%s\n' "$MEM_TEXT_OUT" >&2
  R0=
  return 0
}

imp_on_info() {
  if (( SUPPRESS_INFO )); then
    R0=
    return 0
  fi
  mem_to_text "$1" "$2"
  printf '%s\n' "$MEM_TEXT_OUT" >&2
  R0=
  return 0
}

# Save games are stubbed: at 34s/tick nobody is going to sit through a
# save/load round-trip, and file-backed saves (see ../ruby/../python) would
# just add untested code paths to a demo whose whole point is elsewhere.
# sizeOfSaveGame reporting 0 makes DOOM treat every save slot as empty;
# writeSaveGame silently discards. See README.md for the rationale.
imp_size_of_save() { R0=0; return 0; }
imp_read_save() { R0=0; return 0; }
imp_write_save() { R0=0; return 0; }

# DOOM's internal 35Hz pacing is driven by this, so it has to be a real
# clock, not a fake stepped one. EPOCHREALTIME is "seconds.microseconds";
# millisecond resolution is plenty and avoids a floating-point dependency
# bash doesn't have. `10#` forces base-10 so a leading-zero microseconds
# field (e.g. "007123") isn't parsed as an (invalid) octal literal.
imp_time_ms() {
  local t=$EPOCHREALTIME
  local sec=${t%.*} frac=${t#*.}
  R0=$(( sec * 1000 + 10#${frac:0:3} ))
  return 0
}

# Only records where the framebuffer lives; doom_mem is already a global
# array, so there is nothing to copy out of it (unlike ../ruby/../python,
# which snapshot the buffer here because their host languages don't share
# address space with the module's memory representation).
imp_draw_frame() {
  FRAME_BUF_OFF=$1
  R0=
  return 0
}

# Leaving both output slots untouched selects the wasm-embedded shareware
# WAD; external WADs are out of scope for this frontend (see ../ruby).
imp_wad_sizes() { R0=; return 0; }
imp_read_wads() { R0=; return 0; }

imp_on_game_init() {
  FRAME_W=$1
  FRAME_H=$2
  R0=
  return 0
}

# Read by rt_resolve_import in the sourced doom_gen.sh (IMPORTS[mod.name]),
# not anywhere in this script — hence the unused-variable suppression.
# shellcheck disable=SC2034
declare -A IMPORTS=(
  ['console.onErrorMessage']=imp_on_error
  ['console.onInfoMessage']=imp_on_info
  ['gameSaving.sizeOfSaveGame']=imp_size_of_save
  ['gameSaving.readSaveGame']=imp_read_save
  ['gameSaving.writeSaveGame']=imp_write_save
  ['runtimeControl.timeInMilliseconds']=imp_time_ms
  ['ui.drawFrame']=imp_draw_frame
  ['loading.wadSizes']=imp_wad_sizes
  ['loading.readWads']=imp_read_wads
  ['loading.onGameInit']=imp_on_game_init
)

# ms-resolution monotonic-ish timestamp for phase/tick timing (not the
# game clock -- that's imp_time_ms above); no subshell, unlike `date`.
epoch_ms() {
  local t=$EPOCHREALTIME
  local sec=${t%.*} frac=${t#*.}
  EPOCH_MS_OUT=$(( sec * 1000 + 10#${frac:0:3} ))
}

fmt_secs() {
  local ms=$1
  printf '%d.%03d' $(( ms / 1000 )) $(( ms % 1000 ))
}

# Calls a doom_* export and exits (after restoring the terminal, a no-op
# outside interactive mode) on anything but success -- the status-cascade
# convention's TRAP_MSG is the only place the actual reason lives.
invoke_or_die() {
  local name=$1
  shift
  local status
  doom_invoke "$name" "$@"
  status=$?
  if (( status != 0 )); then
    restore_terminal
    echo "doom (bash): $name failed (status $status): ${TRAP_MSG-unknown error}" >&2
    exit 1
  fi
}

echo "doom (bash): sourcing doom_gen.sh (~19MB generated source, ~1s)..." >&2
epoch_ms; T_SOURCE_START=$EPOCH_MS_OUT
# shellcheck source=/dev/null
source ./doom_gen.sh
epoch_ms; T_SOURCE_END=$EPOCH_MS_OUT
echo "doom (bash): sourced in $(fmt_secs $((T_SOURCE_END - T_SOURCE_START)))s" >&2

# Fits a render grid to a terminal (or a synthetic size for --smoke): one
# character cell shows two vertically-stacked source pixels, so cell rows
# are half of sampled logical-pixel rows. Capped at 160 columns -- far
# short of DOOM's native 320-wide resolution, but 640x400 sampled at 320
# would be 128000 byte reads/frame against a render budget this script
# wants to keep under a second of a 34-SECOND tick, and 160 columns is
# already wider than most terminals people actually run this in.
compute_grid() {
  local term_rows=$1 term_cols=$2 w=$3 h=$4
  local avail_rows=$(( term_rows - 1 )) # one row reserved for the status line
  (( avail_rows < 1 )) && avail_rows=1
  local cap_cols=160
  (( cap_cols > w / 2 )) && cap_cols=$(( w / 2 ))
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
# Deliberately flat (no helper-function calls, no command substitution,
# no per-cell printf) inside the nested loops -- string concatenation
# (+=) and arithmetic ($(( ))) only, because either of the first two is
# the standard way to make a bash render loop slow. There is no
# frame-to-frame diffing (compare ../ruby, which needs it): at ~34s/tick a
# full redraw every frame costs nothing next to the tick itself. An SGR
# code is still skipped when it repeats the previous cell's, which is
# cheap and shrinks the string a lot on DOOM's large flat-color areas
# (its software renderer is paletted, VGA Mode 13h, <=256 colors).
render_frame() {
  local buf="" cy cx top_base bot_base src_x to bo
  local tb tg tr bb bgc br fg_key bg_key
  local last_fg=-1 last_bg=-1
  local w=$FRAME_W h=$FRAME_H off=$FRAME_BUF_OFF
  for (( cy = 0; cy < GRID_ROWS; cy++ )); do
    buf+="${ESC}[$(( cy + 1 ));1H"
    top_base=$(( ((cy * 2) * h / PIXEL_ROWS) * w ))
    bot_base=$(( ((cy * 2 + 1) * h / PIXEL_ROWS) * w ))
    for (( cx = 0; cx < GRID_COLS; cx++ )); do
      src_x=$(( cx * w / GRID_COLS ))
      to=$(( off + (top_base + src_x) * 4 ))
      bo=$(( off + (bot_base + src_x) * 4 ))
      # Memory byte order per pixel is B,G,R,A. Associative-array
      # subscripts are literal strings, not arithmetic expressions (unlike
      # inside `$(( ))`), so every one of these needs an explicit `$`/
      # `$(( ))` -- a bare `doom_mem[to]` would silently look up the key
      # "to" instead of the key named by the variable's value.
      tb=${doom_mem[$to]-0}; tg=${doom_mem[$((to + 1))]-0}; tr=${doom_mem[$((to + 2))]-0}
      bb=${doom_mem[$bo]-0}; bgc=${doom_mem[$((bo + 1))]-0}; br=${doom_mem[$((bo + 2))]-0}
      fg_key=$(( (tr << 16) | (tg << 8) | tb ))
      bg_key=$(( (br << 16) | (bgc << 8) | bb ))
      if (( fg_key != last_fg )); then
        buf+="${ESC}[38;2;${tr};${tg};${tb}m"
        last_fg=$fg_key
      fi
      if (( bg_key != last_bg )); then
        buf+="${ESC}[48;2;${br};${bgc};${bb}m"
        last_bg=$bg_key
      fi
      buf+="▀"
    done
  done
  printf -v RENDER_OUT '%s' "$buf"
}

# Dumps the same sampled grid render_frame uses (GRID_COLS x PIXEL_ROWS
# logical pixels, e.g. 160x100 -- not the full 640x400 framebuffer, which
# would take minutes at this array's access cost) as an ASCII PPM (P3):
# text, so there's no risk of a stray NUL confusing anything downstream,
# unlike a binary P6.
write_ppm() {
  local path=$1
  local w=$FRAME_W h=$FRAME_H off=$FRAME_BUF_OFF
  local py px row_base src_x o r g b
  local buf; buf=$'P3\n'"${GRID_COLS} ${PIXEL_ROWS}"$'\n255\n'
  declare -A distinct=()
  for (( py = 0; py < PIXEL_ROWS; py++ )); do
    row_base=$(( (py * h / PIXEL_ROWS) * w ))
    for (( px = 0; px < GRID_COLS; px++ )); do
      src_x=$(( px * w / GRID_COLS ))
      o=$(( off + (row_base + src_x) * 4 ))
      b=${doom_mem[$o]-0}; g=${doom_mem[$((o + 1))]-0}; r=${doom_mem[$((o + 2))]-0}
      buf+="$r $g $b"$'\n'
      distinct["$r,$g,$b"]=1
    done
  done
  printf '%s' "$buf" > "$path"
  DISTINCT_COLORS=${#distinct[@]}
}

# --- Input (interactive mode only) -----------------------------------
#
# Terminals deliver key *presses* only, never releases. At ~34s/tick there
# is no meaningful "held key" the way ../ruby's timer-based hold models
# one, so a press is instead held down for exactly the tick it was seen
# before: reportKeyDown fires just before tickGame, reportKeyUp just
# after, for every key seen since the previous tick.
declare -a DOWN_KEYS=()
declare -A DOWN_SEEN=()
QUIT=0

add_key() {
  local code=$1
  if [[ ! -v DOWN_SEEN[$code] ]]; then
    DOWN_SEEN[$code]=1
    DOWN_KEYS+=("$code")
  fi
}

# See ../ruby/README.md's table for the mapping this mirrors (Ctrl isn't
# deliverable through a terminal, so fire is 'f'; Shift/run has no
# terminal equivalent and isn't mapped).
handle_char() {
  local c=$1
  case "$c" in
    $'\x03' | q | Q) QUIT=1 ;;
    $'\x0d') add_key "$KEY_ENTER" ;;
    $'\x09') add_key "$KEY_TAB" ;;
    $'\x7f' | $'\x08') add_key "$KEY_BACKSPACE" ;;
    ',') add_key "$KEY_STRAFE_L" ;;
    '.') add_key "$KEY_STRAFE_R" ;;
    f | F) add_key "$KEY_FIRE" ;;
    ' ') add_key "$KEY_USE" ;;
    [a-zA-Z0-9])
      local lc=${c,,} code
      printf -v code '%d' "'$lc"
      add_key "$code"
      ;;
    *) ;;
  esac
}

# Drains whatever bytes have queued up on stdin since the last call (which
# for this frame rate is "since up to 34 seconds ago") and turns them into
# DOWN_KEYS. Arrow keys arrive as ESC [ A/B/C/D; a lone ESC (nothing follows
# within one more 10ms read) is the Escape key itself.
drain_input() {
  DOWN_KEYS=()
  DOWN_SEEN=()
  local ch c2 c3
  while IFS= read -rs -t 0.01 -n 1 ch; do
    if [[ $ch == "$ESC" ]]; then
      if IFS= read -rs -t 0.01 -n 1 c2; then
        if [[ $c2 == '[' ]]; then
          if IFS= read -rs -t 0.01 -n 1 c3; then
            case "$c3" in
              A) add_key "$KEY_UP" ;;
              B) add_key "$KEY_DOWN" ;;
              C) add_key "$KEY_RIGHT" ;;
              D) add_key "$KEY_LEFT" ;;
              *) : ;; # an unrecognized CSI final byte (e.g. an F-key): drop it
            esac
          fi
        else
          # Not a CSI sequence: the ESC was a real Escape keypress, and c2
          # is itself a fresh byte to process, not part of a sequence.
          add_key "$KEY_ESCAPE"
          handle_char "$c2"
        fi
      else
        add_key "$KEY_ESCAPE"
      fi
    else
      handle_char "$ch"
    fi
  done
}

run_smoke() {
  echo "smoke: doom_init (module instantiation + data segments; measures ~25s on the reference machine)..."
  epoch_ms; local t0=$EPOCH_MS_OUT
  doom_init
  local status=$?
  (( status != 0 )) && { echo "smoke: FAIL: doom_init (status $status): ${TRAP_MSG-unknown error}" >&2; exit 1; }
  epoch_ms; local t1=$EPOCH_MS_OUT
  echo "smoke: doom_init in $(fmt_secs $((t1 - t0)))s"

  echo "smoke: initGame (measures ~103s / ~1.7min on the reference machine -- this is expected, not a hang)..."
  epoch_ms; t0=$EPOCH_MS_OUT
  invoke_or_die initGame
  epoch_ms; t1=$EPOCH_MS_OUT
  echo "smoke: initGame in $(fmt_secs $((t1 - t0)))s"

  if (( FRAME_W == 0 )); then
    echo "smoke: FAIL: loading.onGameInit never fired" >&2
    exit 1
  fi
  echo "smoke: onGameInit reported a ${FRAME_W}x${FRAME_H} framebuffer"

  local i
  for (( i = 1; i <= 2; i++ )); do
    echo "smoke: tickGame #$i (measures ~34s on the reference machine)..."
    epoch_ms; t0=$EPOCH_MS_OUT
    invoke_or_die tickGame
    epoch_ms; t1=$EPOCH_MS_OUT
    echo "smoke: tick $i in $(fmt_secs $((t1 - t0)))s"
  done

  # A synthetic 160x51 terminal, matching the cap this frontend always
  # applies (see compute_grid) -- runs the same whether or not stdout is a
  # real tty, which is the point of a headless check.
  compute_grid 51 160 "$FRAME_W" "$FRAME_H"

  epoch_ms; t0=$EPOCH_MS_OUT
  render_frame
  epoch_ms; t1=$EPOCH_MS_OUT
  echo "smoke: rendered one ${GRID_COLS}x${GRID_ROWS}-cell frame (sampled from a ${GRID_COLS}x${PIXEL_ROWS} logical-pixel grid, not the full ${FRAME_W}x${FRAME_H} framebuffer) in $(fmt_secs $((t1 - t0)))s -- rendered to a string only, never drawn to a screen"

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
  # DOOM's software renderer is paletted (classic VGA Mode 13h: at most 256
  # colors), so a healthy frame lands in the low hundreds at most, not the
  # thousands a truecolor renderer would produce; a degenerate frame
  # (blank/solid) lands in the single digits.
  if (( DISTINCT_COLORS <= 50 )); then
    echo "smoke: FAIL: frame looks degenerate (too few distinct colors)" >&2
    exit 1
  fi

  echo "smoke: PASS"
}

run_interactive() {
  if [[ ! -t 0 || ! -t 1 ]]; then
    echo "doom (bash): interactive mode needs a real terminal on stdin/stdout (try \`./main.sh --smoke\` for a headless check)" >&2
    exit 1
  fi

  # The alternate screen is entered immediately, before doom_init/initGame
  # even start, rather than only once the game loop is ready: boot alone
  # is ~2 minutes (see README.md), and that's a much better two minutes to
  # spend already inside the alternate screen -- printing plain scrolling
  # progress lines onto it -- than leaving the real scrollback cluttered
  # and then cutting over. It also means Ctrl-C/kill during boot exercises
  # the same restore path as quitting from the game loop, not a separate
  # untested one.
  ORIG_STTY=$(stty -g)
  restore_terminal() {
    stty "$ORIG_STTY" 2>/dev/null || true
    printf '%s' "${ESC}[?25h${ESC}[?1049l"
  }
  # Ctrl-C is handled explicitly as a byte in drain_input (once raw mode
  # is active) because raw mode disables the terminal's own SIGINT
  # generation; the INT/TERM traps are only a backstop for termination
  # from outside (e.g. `kill`) or during boot before drain_input ever
  # runs. Unlike EXIT, a custom INT/TERM trap does not itself end the
  # process -- bash resumes whatever it was doing once the handler
  # returns -- so these have to call exit explicitly (which then also
  # fires the EXIT trap; restore_terminal is idempotent, so running it
  # twice is harmless).
  trap restore_terminal EXIT
  trap 'restore_terminal; exit 130' INT
  trap 'restore_terminal; exit 143' TERM
  stty raw -echo
  printf '%s' "${ESC}[?1049h${ESC}[?25l${ESC}[2J${ESC}[H"
  # -echo means a plain `printf`'d line won't start at column 1 of the
  # next row on its own; \r\n (not bare \n) keeps boot progress readable.
  boot_msg() { printf '%s\r\n' "$1"; }

  boot_msg "dewasm DOOM (bash) -- booting. This takes about two minutes total; none of this is a hang."
  boot_msg "instantiating module (data segments; measures ~25s on the reference machine)..."
  doom_init
  local status=$?
  if (( status != 0 )); then
    restore_terminal
    echo "doom (bash): doom_init failed (status $status): ${TRAP_MSG-unknown error}" >&2
    exit 1
  fi

  boot_msg "running initGame -- measures ~103s (~1.7min) on the reference machine..."
  invoke_or_die initGame
  boot_msg "initGame done, ${FRAME_W}x${FRAME_H} framebuffer. Each tick is ~34s -- press Enter a few times, then wait; that's what gets you past the title screen."

  doom_global_get KEY_UPARROW; KEY_UP=$R0
  doom_global_get KEY_DOWNARROW; KEY_DOWN=$R0
  doom_global_get KEY_LEFTARROW; KEY_LEFT=$R0
  doom_global_get KEY_RIGHTARROW; KEY_RIGHT=$R0
  doom_global_get KEY_STRAFE_L; KEY_STRAFE_L=$R0
  doom_global_get KEY_STRAFE_R; KEY_STRAFE_R=$R0
  doom_global_get KEY_FIRE; KEY_FIRE=$R0
  doom_global_get KEY_USE; KEY_USE=$R0
  doom_global_get KEY_TAB; KEY_TAB=$R0
  doom_global_get KEY_ESCAPE; KEY_ESCAPE=$R0
  doom_global_get KEY_ENTER; KEY_ENTER=$R0
  doom_global_get KEY_BACKSPACE; KEY_BACKSPACE=$R0

  local term_rows term_cols
  read -r term_rows term_cols < <(stty size 2>/dev/null) || { term_rows=${LINES:-24}; term_cols=${COLUMNS:-80}; }
  compute_grid "$term_rows" "$term_cols" "$FRAME_W" "$FRAME_H"
  printf '%s' "${ESC}[2J${ESC}[H" # clear the boot-progress scrollback before the first game frame

  local tick=0
  local dt_ms=0 code
  while (( QUIT == 0 )); do
    drain_input
    (( QUIT )) && break

    for code in "${DOWN_KEYS[@]}"; do invoke_or_die reportKeyDown "$code"; done

    epoch_ms; local t0=$EPOCH_MS_OUT
    invoke_or_die tickGame
    epoch_ms; local t1=$EPOCH_MS_OUT
    dt_ms=$(( t1 - t0 ))

    for code in "${DOWN_KEYS[@]}"; do invoke_or_die reportKeyUp "$code"; done

    (( tick++ ))
    render_frame
    local status_text
    status_text="dewasm DOOM (bash) | tick $tick | $(fmt_secs "$dt_ms")s/tick | q quit  f fire  space use  arrows move  ,/. strafe  tab automap  enter confirm"
    printf '%s' "$RENDER_OUT${ESC}[$(( GRID_ROWS + 1 ));1H${ESC}[K${status_text}"
  done
}

if [[ $MODE == smoke ]]; then
  run_smoke
else
  run_interactive
fi
