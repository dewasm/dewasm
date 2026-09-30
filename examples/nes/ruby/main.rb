#!/usr/bin/env ruby
# frozen_string_literal: true

# Interactive terminal frontend for the dewasm-generated NES library.
# `build.sh` produces that library (`nes_gen.rb`) from `cache/nes.wasm`, which is built on `agnes`.
# Unlike `../../doom`, `nes.wasm` has zero host imports, so there's nothing to connect.
# This frontend only has to load a ROM into the module's linear memory.
# Then it drives the game loop itself.
# Pacing, input polling, and rendering all belong wholly to the host.
# In DOOM, the module paces itself off a `timeInMilliseconds` import instead.
#
# Rendering reuses the half-block 24-bit color trick of `../../doom/ruby`.
# This is not a step down even from a pixel window.
# A terminal has orders of magnitude fewer cells to redraw than a window has pixels.
# It also reuses the key-hold heuristic for terminals delivering only key presses.
#
# Run with `--smoke` for a headless self-check (no terminal needed).
# It initializes the game and ticks it a few hundred times with no input.
# It measures tick rate and render cost.
# Then it writes the final frame to screenshot.ppm.

require_relative "nes_gen"
require "io/console"

# Terminals deliver only key *presses*.
# So a press is held "down" for this long after the last matching press/key repeat.
# Then it drops out of the `setInput` bit mask.
# The hold is comfortably above a terminal's own key repeat interval.
KEY_HOLD_SECONDS = 0.18

DEFAULT_ROM_PATH = File.join(__dir__, "..", "..", "apps", "cache", "alter_ego.nes")

# Button bits accepted by the module's `setInput` export (`examples/apps/src/nes_demo.c`).
BUTTON_BITS = {
  a: 0x01,
  b: 0x02,
  select: 0x04,
  start: 0x08,
  up: 0x10,
  down: 0x20,
  left: 0x40,
  right: 0x80,
}.freeze

# Read the ROM file, copy it into the module's linear memory via `allocRom`.
# Then initialize the emulator.
# Raises if the ROM is rejected (unsupported mapper or damaged file).
# That is the module's own way of signalling failure.
def load_rom(nes, path)
  rom = File.binread(path)
  ptr = nes.invoke("allocRom", rom.bytesize)
  nes.memory.buffer.set_string(rom, ptr, rom.bytesize, 0)
  ok = nes.invoke("initGame")
  raise "nes: initGame failed for #{path} (ROM rejected or unsupported mapper)" unless ok == 1
end

# Renders the frame into ANSI half-block terminal cells.
# Each character cell shows two vertically-stacked source pixels via "▀".
# The foreground is the top pixel and the background the bottom pixel, both 24-bit color SGR.
# This is the same trick as the `Renderer` of `../../doom/ruby`, ported here with one difference.
# The NES's 256x240 framebuffer is already native resolution.
# It has no implicit 2x upscale like DOOM's 640x400.
# So the pixel cap is the frame's own width, not half of it.
# This diff and escape-sequence tracking is the performance-sensitive part of this frontend.
# The wasm execution is not.
# It diffs against the previous frame's cell contents and the terminal's own cursor position.
# It only emits an SGR code when a cell's color actually changed.
#
# The guest hands over palette *indices*, not colors, which suits this renderer exactly.
# A terminal cell samples one pixel out of several.
# So the only palette lookups performed are the sampled ones.
# A color is a function of its index.
# So the whole SGR string per index is computed once.
# The frame diff then compares indices directly.
# Fixed status-line colors (white on black), independent of the game's own palette.
# Without an explicit color the status line inherits the colors the last-drawn pixel cell left.
# It would then flicker with the game.
STATUS_SGR = "\e[48;2;0;0;0m\e[38;2;255;255;255m"

class Renderer
  attr_reader :cell_cols, :cell_rows

  def initialize(term_cols, term_rows, frame_w, frame_h, palette)
    status_rows = 1
    avail_rows = [term_rows - status_rows, 1].max
    pixel_cols = [term_cols, frame_w].min
    pixel_rows = (pixel_cols * frame_h / frame_w.to_f).round
    cell_rows = pixel_rows / 2
    if cell_rows > avail_rows
      cell_rows = avail_rows
      pixel_rows = cell_rows * 2
      pixel_cols = [(pixel_rows * frame_w / frame_h.to_f).round, term_cols, frame_w].min
    end
    @pixel_cols = pixel_cols
    @pixel_rows = pixel_rows
    @cell_cols = pixel_cols
    @cell_rows = cell_rows
    @fg_sgr = palette.map { |r, g, b| "\e[38;2;#{r};#{g};#{b}m" }
    @bg_sgr = palette.map { |r, g, b| "\e[48;2;#{r};#{g};#{b}m" }
    @prev = Array.new(cell_rows) { Array.new(@cell_cols) }
    @cursor_row = nil
    @cursor_col = nil
    @last_fg = nil
    @last_bg = nil
    @last_status = nil
  end

  # Builds one frame's worth of escape sequences/characters as a single string.
  # The caller is responsible for writing it.
  # For `--smoke`, the caller just times how long this took and drops it.
  def render(screen, frame_w, frame_h, status_text)
    buf = String.new(capacity: @cell_cols * @cell_rows * 4)
    @cell_rows.times do |cy|
      top_row_base = ((cy * 2) * frame_h / @pixel_rows) * frame_w
      bot_row_base = ((cy * 2 + 1) * frame_h / @pixel_rows) * frame_w
      prev_row = @prev[cy]
      @cell_cols.times do |cx|
        src_x = cx * frame_w / @pixel_cols
        # One byte per pixel, a palette index.
        # The `& 0x3f` mask is load-bearing (`examples/apps/src/nes_demo.c`).
        top = screen.getbyte(top_row_base + src_x) & 0x3f
        bot = screen.getbyte(bot_row_base + src_x) & 0x3f
        key = (top << 6) | bot
        next if prev_row[cx] == key

        prev_row[cx] = key
        buf << "\e[#{cy + 1};#{cx + 1}H" unless @cursor_row == cy && @cursor_col == cx
        if @last_fg != top
          @last_fg = top
          buf << @fg_sgr[top]
        end
        if @last_bg != bot
          @last_bg = bot
          buf << @bg_sgr[bot]
        end
        buf << "▀"
        @cursor_row = cy
        @cursor_col = cx + 1
      end
    end
    if status_text != @last_status
      # Reset SGR first.
      # Otherwise the status line inherits whichever colors the last-drawn pixel cell left active.
      # Its background would then flicker with the game's own colors.
      # With the reset it stays the terminal default.
      buf << "\e[#{@cell_rows + 1};1H\e[0m#{STATUS_SGR}\e[K#{status_text}"
      @last_status = status_text
      @cursor_row = -1 # force the next painted cell to reposition: the cursor is now on the status line
      @last_fg = nil # the reset above invalidated the SGR cache; force the next cell to re-emit its color
      @last_bg = nil
    end
    buf
  end
end

# Terminals deliver only key *presses*, never releases.
# So a press marks a button held until KEY_HOLD_SECONDS pass with no matching repeat.
# Terminal key repeat just resends the same bytes, which pushes the deadline back.
# DOOM has discrete `reportKeyDown`/`reportKeyUp` events.
# Unlike those, `setInput` wants the full held-button state on every tick.
# So this hands back a bit mask rather than calling anything on the module itself.
class InputHandler
  ESCAPE_SEQUENCES = {
    "\e[A" => :up,
    "\e[B" => :down,
    "\e[C" => :right,
    "\e[D" => :left,
  }.freeze

  def initialize
    @pending = "".b
    @esc_seen_at = nil
    @held_until = {}
    @quit = false
  end

  def quit? = @quit

  def poll(now)
    read_available
    process_pending(now)
    expire_held_keys(now)
  end

  # The `setInput` bit mask for every button currently considered held.
  def mask
    @held_until.keys.reduce(0) { |m, key| m | BUTTON_BITS.fetch(key) }
  end

  private

  def read_available
    loop do
      chunk = $stdin.read_nonblock(64, exception: false)
      break if chunk.nil? || chunk == :wait_readable

      @pending << chunk
    end
  end

  def process_pending(now)
    loop do
      break if @pending.empty?

      if @pending.getbyte(0) == 0x1b
        break unless process_escape(now)

        next
      end

      byte = @pending.getbyte(0)
      @pending = @pending.byteslice(1..)
      handle_byte(byte, now)
    end
  end

  # Returns true if it consumed (or decided to drop) something from `@pending`.
  # Returns false if it needs more bytes and the caller should stop polling for this tick.
  def process_escape(now)
    if @pending.bytesize >= 3
      seq = ESCAPE_SEQUENCES.keys.find { |s| @pending.start_with?(s) }
      if seq
        key_down(ESCAPE_SEQUENCES[seq], now)
        @pending = @pending.byteslice(seq.bytesize..)
      else
        # Not one of our known arrow sequences (for example an F-key or Home/End CSI sequence).
        # Drop just the ESC byte and reprocess the rest as ordinary bytes.
        # That way they are not lost.
        @pending = @pending.byteslice(1..)
      end
      @esc_seen_at = nil
      return true
    end

    if @pending.bytesize == 2 && @pending.getbyte(1) != 0x5b # second byte isn't '['
      @pending = @pending.byteslice(1..)
      @esc_seen_at = nil
      return true
    end

    if @pending == "\e" && @esc_seen_at
      # Still a lone ESC on a second poll with no growth: a real Escape key press.
      # It has no mapping here, so it's just dropped.
      @pending = "".b
      @esc_seen_at = nil
      return true
    end

    # "\e" (seen for the first time) or "\e[" (a valid prefix so far) is not complete yet.
    # Wait for the rest to arrive on a later poll.
    @esc_seen_at ||= now if @pending == "\e"
    false
  end

  def handle_byte(byte, now)
    case byte
    when 0x03, 0x71 # Ctrl-C, 'q'
      @quit = true
    when 0x0d then key_down(:start, now) # Enter
    when 0x20 then key_down(:select, now) # Space
    when 0x78 then key_down(:a, now) # 'x'
    when 0x7a then key_down(:b, now) # 'z'
    end
  end

  def key_down(key, now)
    @held_until[key] = now + KEY_HOLD_SECONDS
  end

  def expire_held_keys(now)
    expired = @held_until.select { |_, deadline| now >= deadline }.keys
    expired.each { |key| @held_until.delete(key) }
  end
end

def check_yjit!
  return if defined?(RubyVM::YJIT) && RubyVM::YJIT.enabled?

  warn "nes: YJIT is not enabled (run with `ruby --yjit`, or set RUBY_YJIT_ENABLE=1) " \
       "- the Ruby backend is already dewasm's slowest, and needs YJIT to stay playable."
end

def write_ppm(path, w, h, screen, palette)
  rgb = []
  screen.each_byte { |ix| rgb.concat(palette[ix & 0x3f]) }
  File.open(path, "wb") do |f|
    f.write("P6\n#{w} #{h}\n255\n")
    f.write(rgb.pack("C*"))
  end
  File.expand_path(path)
end

# The module's fixed 64-entry palette (R,G,B,A per entry; alpha is padding).
# It is read once: it never changes, unlike the per-frame index buffer.
def read_palette(nes)
  bytes = nes.memory.buffer.get_string(nes.invoke("paletteOffset"), 64 * 4).bytes
  Array.new(64) { |i| bytes[i * 4, 3] }
end

def new_nes
  nes = Nes.new
  # Reactor initialization comes before any other export.
  # `nes.wasm` has no WASI surface to run it implicitly.
  nes.invoke("_initialize")
  nes
end

def run_smoke(rom_path)
  nes = new_nes
  load_rom(nes, rom_path)

  frame_w = nes.invoke("frameWidth")
  frame_h = nes.invoke("frameHeight")
  screen_off = nes.invoke("screenOffset")
  palette = read_palette(nes)

  # A synthetic terminal size, so this runs in CI/anywhere with no real terminal.
  renderer = Renderer.new(160, 51, frame_w, frame_h, palette)

  ticks = 300
  render_seconds = 0.0
  start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  ticks.times do
    nes.invoke("setInput", 0)
    nes.invoke("tickGame")
    r0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    screen = nes.memory.buffer.get_string(screen_off, frame_w * frame_h)
    renderer.render(screen, frame_w, frame_h, "smoke")
    render_seconds += Process.clock_gettime(Process::CLOCK_MONOTONIC) - r0
  end
  elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - start
  bare_rate = ticks / (elapsed - render_seconds)
  full_rate = ticks / elapsed
  render_ms = render_seconds / ticks * 1000
  puts format(
    "smoke: ran %d ticks in %.3fs - %.1f ticks/sec bare, %.1f ticks/sec with terminal rendering (%.3fms/frame render)",
    ticks, elapsed, bare_rate, full_rate, render_ms
  )

  screen = nes.memory.buffer.get_string(screen_off, frame_w * frame_h)
  distinct = {}
  screen.each_byte { |ix| distinct[palette[ix & 0x3f]] = true }
  puts "smoke: final frame is #{frame_w}x#{frame_h} with #{distinct.size} distinct colors"
  # The NES PPU palette tops out at 64 colors total, so a healthy frame lands in the dozens.
  # A degenerate (blank/solid) frame lands in the single digits.
  # This mirrors the >4 threshold the snapshot oracle uses (`crates/xtask/src/nes_snapshot.rs`).
  if distinct.size <= 4
    warn "smoke: FAIL: frame looks degenerate (too few distinct colors)"
    exit 1
  end

  path = write_ppm("screenshot.ppm", frame_w, frame_h, screen, palette)
  puts "smoke: wrote #{path}"
end

ENTER_ALT_SCREEN = "\e[?1049h\e[?25l\e[2J\e[H"
# SGR reset first.
# Otherwise the fixed status-line colors persist past leaving the alternate screen.
# They would color the shell prompt underneath.
EXIT_ALT_SCREEN = "\e[0m\e[?25h\e[?1049l"

# The NTSC NES runs at ~60.0988Hz.
# 60 exactly is close enough that no separate calibration is needed.
# DOOM differs: the host has no say at all over its internal 35Hz pacing.
TARGET_FPS = 60
FRAME_SECONDS = 1.0 / TARGET_FPS

def run_interactive(rom_path)
  unless $stdin.tty? && $stdout.tty? && IO.console
    raise "nes: interactive mode needs a real terminal on stdin/stdout (try `./run.sh --smoke` for a headless check)"
  end

  nes = new_nes
  load_rom(nes, rom_path)
  frame_w = nes.invoke("frameWidth")
  frame_h = nes.invoke("frameHeight")
  screen_off = nes.invoke("screenOffset")
  palette = read_palette(nes)

  rows, cols = IO.console.winsize
  renderer = Renderer.new(cols, rows, frame_w, frame_h, palette)
  input = InputHandler.new

  restored = false
  restore = lambda do
    next if restored

    restored = true
    $stdout.write(EXIT_ALT_SCREEN)
    $stdout.flush
  end
  at_exit(&restore)
  # Ctrl-C is handled explicitly as a byte in InputHandler.
  # Raw mode disables the terminal's own SIGINT generation.
  # These traps are only a fallback for ending the process from outside (for example `kill`).
  Signal.trap("INT") { restore.call; exit(0) }
  Signal.trap("TERM") { restore.call; exit(0) }

  $stdout.write(ENTER_ALT_SCREEN)
  $stdout.flush
  begin
    $stdin.raw do
      status_window_start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      status_ticks = 0
      status_text = "dewasm NES | starting... | q/^C quit  arrows d-pad  x=A z=B  enter=start  space=select"
      # Pacing with a fixed time step: the module has no clock import of its own.
      # So 60Hz is entirely the host's job.
      # Cap at 60 ticks/sec by sleeping when ahead of schedule.
      # When the interpreter can't keep up, never sleep and just resync the schedule to "now".
      # That avoids trying to catch up on all the missed frames.
      next_frame_at = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      loop do
        now = Process.clock_gettime(Process::CLOCK_MONOTONIC)
        input.poll(now)
        break if input.quit?

        nes.invoke("setInput", input.mask)
        nes.invoke("tickGame")
        status_ticks += 1
        elapsed = now - status_window_start
        if elapsed >= 0.5
          rate = status_ticks / elapsed
          status_text = format(
            "dewasm NES | %.1f ticks/sec | q/^C quit  arrows d-pad  x=A z=B  enter=start  space=select", rate
          )
          status_ticks = 0
          status_window_start = now
        end

        screen = nes.memory.buffer.get_string(screen_off, frame_w * frame_h)
        $stdout.write(renderer.render(screen, frame_w, frame_h, status_text))

        next_frame_at += FRAME_SECONDS
        now2 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
        if next_frame_at > now2
          sleep(next_frame_at - now2)
        else
          next_frame_at = now2
        end
      end
    end
  ensure
    restore.call
  end
end

check_yjit!
argv = ARGV.dup
smoke = argv.delete("--smoke") ? true : false
rom_path = argv.first || DEFAULT_ROM_PATH
if smoke
  run_smoke(rom_path)
else
  run_interactive(rom_path)
end
