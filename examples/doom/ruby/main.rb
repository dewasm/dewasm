#!/usr/bin/env ruby
# frozen_string_literal: true

# Interactive terminal frontend for the dewasm-generated DOOM library
# (doom_gen.rb, produced from jacobenget/doom.wasm by build.sh).
# Instead of a pixel window (see ../go, ../java) this renders into any ANSI truecolor terminal with half-block characters: the Ruby backend only manages
# ~15 ticks/sec under YJIT, far below what a GUI needs but plenty for a terminal, which has orders of magnitude fewer cells to redraw than a window has pixels.
#
# Run with --smoke for a headless self-check (no tty needed): it inits the game, ticks it 60 times, measures tick rate and render cost, and writes the final frame to screenshot.ppm.

require_relative "doom_gen"
require "io/console"

SAVE_DIR = ".savegame"
# Terminals deliver only key *presses*, so a press is held "down" for this long after the last matching press/autorepeat before synthesizing the release; comfortably above a terminal's own autorepeat interval.
KEY_HOLD_SECONDS = 0.18

def save_game_path(id)
  File.join(SAVE_DIR, "doomsav#{id}.dsg")
end

# Wires the wasm module's ten host imports to Ruby.
# `doom_holder` exists because these closures have to be built before Doom.new returns the instance they read memory from; it's filled in immediately after construction and only read from within calls the imports themselves receive later (never during Doom.new itself).
def build_imports(doom_holder, frame_state, suppress_info:)
  {
    "console" => {
      "onErrorMessage" => lambda do |off, len|
        warn doom_holder[0].memory.buffer.get_string(off, len)
      end,
      "onInfoMessage" => lambda do |off, len|
        # Info messages would corrupt the ANSI frame while the alternate screen is active, so they're dropped in interactive mode;
        # --smoke has no alternate screen and prints them normally.
        next if suppress_info

        puts doom_holder[0].memory.buffer.get_string(off, len)
      end,
    },
    "gameSaving" => {
      "sizeOfSaveGame" => lambda do |id|
        path = save_game_path(id)
        File.exist?(path) ? File.size(path) : 0
      end,
      "readSaveGame" => lambda do |id, dst_off|
        path = save_game_path(id)
        next 0 unless File.exist?(path)

        bytes = File.binread(path)
        doom_holder[0].memory.buffer.set_string(bytes, dst_off, bytes.bytesize, 0)
        bytes.bytesize
      end,
      "writeSaveGame" => lambda do |id, src_off, length|
        Dir.mkdir(SAVE_DIR) unless Dir.exist?(SAVE_DIR)
        bytes = doom_holder[0].memory.buffer.get_string(src_off, length)
        File.binwrite(save_game_path(id), bytes)
        length
      end,
    },
    "runtimeControl" => {
      # Backs DOOM's internal 35Hz pacing, so it has to be a real monotonic clock (not a fake stepped one) or the game's notion of elapsed time would drift from how often we actually call tickGame.
      "timeInMilliseconds" => lambda do
        Process.clock_gettime(Process::CLOCK_MONOTONIC, :millisecond)
      end,
    },
    "ui" => {
      # Captured as one immediate bulk copy (IO::Buffer#get_string), not scanned pixel-by-pixel: a per-pixel wasm memory call here would be
      # ~256k calls/frame and would dominate the whole tick budget.
      "drawFrame" => lambda do |buf_off|
        len = frame_state[:width] * frame_state[:height] * 4
        frame_state[:pixels] = doom_holder[0].memory.buffer.get_string(buf_off, len)
      end,
    },
    "loading" => {
      "onGameInit" => lambda do |w, h|
        frame_state[:width] = w
        frame_state[:height] = h
      end,
      # Leaving both output slots untouched (they arrive pre-zeroed) selects the wasm-embedded shareware WAD; supplying external WADs is out of scope for this frontend.
      "wadSizes" => lambda { |_num_off, _bytes_off| },
      "readWads" => lambda { |_dst_off, _lengths_off| },
    },
  }
end

def key_map(doom)
  {
    up: doom.global_get("KEY_UPARROW"),
    down: doom.global_get("KEY_DOWNARROW"),
    left: doom.global_get("KEY_LEFTARROW"),
    right: doom.global_get("KEY_RIGHTARROW"),
    strafe_l: doom.global_get("KEY_STRAFE_L"),
    strafe_r: doom.global_get("KEY_STRAFE_R"),
    fire: doom.global_get("KEY_FIRE"),
    use: doom.global_get("KEY_USE"),
    tab: doom.global_get("KEY_TAB"),
    escape: doom.global_get("KEY_ESCAPE"),
    enter: doom.global_get("KEY_ENTER"),
    backspace: doom.global_get("KEY_BACKSPACE"),
    # KEY_SHIFT/KEY_ALT exist as exports too, but Shift (run) and Alt have no terminal-deliverable equivalent, so they're never looked up here.
  }
end

# Renders the framebuffer into ANSI half-block terminal cells: each character cell shows two vertically-stacked source pixels via "▀"
# (foreground = top pixel, background = bottom pixel, both 24-bit truecolor
# SGR).
# This is the performance-sensitive part of this frontend, not the wasm execution, so it diffs against the previous frame's cell contents and its own idea of where the terminal's cursor already sits, and only emits an SGR code when a cell's color actually differs from the one before it: DOOM's software renderer is paletted, so most cells repeat exactly from one frame to the next.
# Fixed status-line colors (white on black), independent of the game's own palette -- without an explicit color the status line inherits whatever fg/bg the last-drawn pixel cell left active, flickering with the game.
STATUS_SGR = "\e[48;2;0;0;0m\e[38;2;255;255;255m"

class Renderer
  attr_reader :cell_cols, :cell_rows

  def initialize(term_cols, term_rows, frame_w, frame_h)
    status_rows = 1
    avail_rows = [term_rows - status_rows, 1].max
    # 320x200 (DOOM's native, pre-2x-upscale resolution) is the natural cap:
    # sampling beyond one source pixel per 2 is not showing any more detail.
    pixel_cols = [term_cols, frame_w / 2].min
    pixel_rows = (pixel_cols * frame_h / frame_w.to_f).round
    cell_rows = pixel_rows / 2
    if cell_rows > avail_rows
      cell_rows = avail_rows
      pixel_rows = cell_rows * 2
      pixel_cols = [(pixel_rows * frame_w / frame_h.to_f).round, term_cols, frame_w / 2].min
    end
    @pixel_cols = pixel_cols
    @pixel_rows = pixel_rows
    @cell_cols = pixel_cols
    @cell_rows = cell_rows
    @prev = Array.new(cell_rows) { Array.new(@cell_cols) }
    @cursor_row = nil
    @cursor_col = nil
    @last_fg = nil
    @last_bg = nil
    @last_status = nil
  end

  # Builds one frame's worth of escape sequences/characters as a single string; the caller is responsible for writing it (or, for --smoke, just timing how long this took and discarding it).
  def render(pixels, frame_w, frame_h, status_text)
    buf = String.new(capacity: @cell_cols * @cell_rows * 4)
    @cell_rows.times do |cy|
      top_row_base = ((cy * 2) * frame_h / @pixel_rows) * frame_w
      bot_row_base = ((cy * 2 + 1) * frame_h / @pixel_rows) * frame_w
      prev_row = @prev[cy]
      @cell_cols.times do |cx|
        src_x = cx * frame_w / @pixel_cols
        to = (top_row_base + src_x) * 4
        bo = (bot_row_base + src_x) * 4
        # Memory byte order is B,G,R,A (see loading.onGameInit callers).
        top_r = pixels.getbyte(to + 2)
        top_g = pixels.getbyte(to + 1)
        top_b = pixels.getbyte(to)
        bot_r = pixels.getbyte(bo + 2)
        bot_g = pixels.getbyte(bo + 1)
        bot_b = pixels.getbyte(bo)
        key = (top_r << 40) | (top_g << 32) | (top_b << 24) | (bot_r << 16) | (bot_g << 8) | bot_b
        next if prev_row[cx] == key

        prev_row[cx] = key
        buf << "\e[#{cy + 1};#{cx + 1}H" unless @cursor_row == cy && @cursor_col == cx
        fg = key >> 24
        if @last_fg != fg
          @last_fg = fg
          buf << "\e[38;2;#{top_r};#{top_g};#{top_b}m"
        end
        bg = key & 0xffffff
        if @last_bg != bg
          @last_bg = bg
          buf << "\e[48;2;#{bot_r};#{bot_g};#{bot_b}m"
        end
        buf << "▀"
        @cursor_row = cy
        @cursor_col = cx + 1
      end
    end
    if status_text != @last_status
      # Reset SGR first: otherwise the status line inherits whichever fg/bg the last-drawn pixel cell left active, making its background flicker with the game's own colors instead of staying the terminal default.
      buf << "\e[#{@cell_rows + 1};1H\e[0m#{STATUS_SGR}\e[K#{status_text}"
      @last_status = status_text
      @cursor_row = -1 # force the next painted cell to reposition: the cursor is now on the status line
      @last_fg = nil # the reset above invalidated the SGR cache; force the next cell to re-emit its color
      @last_bg = nil
    end
    buf
  end
end

# Terminals deliver only key *presses*, never releases, so a press synthesizes both an immediate reportKeyDown and a reportKeyUp once
# KEY_HOLD_SECONDS pass with no matching repeat (terminal autorepeat just resends the same bytes, which pushes the deadline back via #key_down).
class InputHandler
  ESCAPE_SEQUENCES = {
    "\e[A" => :up,
    "\e[B" => :down,
    "\e[C" => :right,
    "\e[D" => :left,
  }.freeze

  def initialize(doom, keys)
    @doom = doom
    @keys = keys
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

  # Returns true if it consumed (or decided to drop) something from
  # @pending, false if it needs more bytes and the caller should stop polling for this tick.
  def process_escape(now)
    if @pending.bytesize >= 3
      seq = ESCAPE_SEQUENCES.keys.find { |s| @pending.start_with?(s) }
      if seq
        key_down(@keys.fetch(ESCAPE_SEQUENCES[seq]), now)
        @pending = @pending.byteslice(seq.bytesize..)
      else
        # Not one of our known arrow sequences (e.g. an F-key or Home/End
        # CSI sequence): drop just the ESC byte and reprocess the rest as ordinary bytes rather than losing them.
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
      # Still a bare ESC on a second poll with no growth: a real Escape key press, not the start of a sequence still in flight.
      key_down(@keys.fetch(:escape), now)
      @pending = "".b
      @esc_seen_at = nil
      return true
    end

    # "\e" (seen for the first time) or "\e[" (a valid prefix so far):
    # wait for the rest to arrive on a later poll.
    @esc_seen_at ||= now if @pending == "\e"
    false
  end

  def handle_byte(byte, now)
    case byte
    when 0x03, 0x71 # Ctrl-C, 'q'
      @quit = true
    when 0x0d then key_down(@keys.fetch(:enter), now)
    when 0x09 then key_down(@keys.fetch(:tab), now)
    when 0x7f then key_down(@keys.fetch(:backspace), now)
    when 0x2c then key_down(@keys.fetch(:strafe_l), now)
    when 0x2e then key_down(@keys.fetch(:strafe_r), now)
    when 0x66 then key_down(@keys.fetch(:fire), now)
    when 0x20 then key_down(@keys.fetch(:use), now)
    else
      c = byte.chr.downcase
      key_down(c.ord, now) if c =~ /[a-z0-9]/
    end
  end

  def key_down(code, now)
    @doom.invoke("reportKeyDown", code)
    @held_until[code] = now + KEY_HOLD_SECONDS
  end

  def expire_held_keys(now)
    expired = @held_until.select { |_, deadline| now >= deadline }.keys
    expired.each do |code|
      @doom.invoke("reportKeyUp", code)
      @held_until.delete(code)
    end
  end
end

def check_yjit!
  return if defined?(RubyVM::YJIT) && RubyVM::YJIT.enabled?

  warn "doom: YJIT is not enabled (run with `ruby --yjit`, or set RUBY_YJIT_ENABLE=1) " \
       "- the Ruby backend is already dewasm's slowest, and needs YJIT to stay playable."
end

def write_ppm(path, w, h, pixels)
  rgb = Array.new(w * h * 3)
  (w * h).times do |i|
    o = i * 4
    j = i * 3
    # Memory byte order is B,G,R,A; PPM wants R,G,B.
    rgb[j] = pixels.getbyte(o + 2)
    rgb[j + 1] = pixels.getbyte(o + 1)
    rgb[j + 2] = pixels.getbyte(o)
  end
  File.open(path, "wb") do |f|
    f.write("P6\n#{w} #{h}\n255\n")
    f.write(rgb.pack("C*"))
  end
  File.expand_path(path)
end

def run_smoke
  frame_state = { width: 0, height: 0, pixels: nil }
  doom_holder = [nil]
  doom = Doom.new(build_imports(doom_holder, frame_state, suppress_info: false))
  doom_holder[0] = doom

  doom.invoke("initGame")
  if frame_state[:width].zero?
    warn "smoke: FAIL: initGame never triggered loading.onGameInit"
    exit 1
  end

  # A synthetic terminal size, so this runs in CI/anywhere with no real tty.
  renderer = Renderer.new(160, 51, frame_state[:width], frame_state[:height])

  ticks = 60
  render_seconds = 0.0
  start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  ticks.times do
    doom.invoke("tickGame")
    r0 = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    renderer.render(frame_state[:pixels], frame_state[:width], frame_state[:height], "smoke")
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

  pixels = frame_state[:pixels]
  unless pixels
    warn "smoke: FAIL: no frame was ever captured"
    exit 1
  end

  w = frame_state[:width]
  h = frame_state[:height]
  distinct = {}
  (w * h).times do |i|
    o = i * 4
    distinct[pixels.getbyte(o) | (pixels.getbyte(o + 1) << 8) | (pixels.getbyte(o + 2) << 16)] = true
  end
  puts "smoke: final frame is #{w}x#{h} with #{distinct.size} distinct colors"
  # DOOM's software renderer is paletted (classic VGA Mode 13h: at most 256 colors), so a healthy frame tops out in the low hundreds, not the thousands a truecolor renderer would produce.
  # A degenerate frame
  # (blank/solid) instead lands in the single digits.
  if distinct.size <= 50
    warn "smoke: FAIL: frame looks degenerate (too few distinct colors)"
    exit 1
  end

  path = write_ppm("screenshot.ppm", w, h, pixels)
  puts "smoke: wrote #{path}"
end

ENTER_ALT_SCREEN = "\e[?1049h\e[?25l\e[2J\e[H"
# SGR reset first: the fixed status-line colors otherwise persist past leaving the alternate screen and tint the shell prompt underneath.
EXIT_ALT_SCREEN = "\e[0m\e[?25h\e[?1049l"

def run_interactive
  unless $stdin.tty? && $stdout.tty? && IO.console
    raise "doom: interactive mode needs a real terminal on stdin/stdout (try `./run.sh --smoke` for a headless check)"
  end

  doom_holder = [nil]
  frame_state = { width: 0, height: 0, pixels: nil }
  doom = Doom.new(build_imports(doom_holder, frame_state, suppress_info: true))
  doom_holder[0] = doom
  doom.invoke("initGame")

  rows, cols = IO.console.winsize
  renderer = Renderer.new(cols, rows, frame_state[:width], frame_state[:height])
  input = InputHandler.new(doom, key_map(doom))

  restored = false
  restore = lambda do
    next if restored

    restored = true
    $stdout.write(EXIT_ALT_SCREEN)
    $stdout.flush
  end
  at_exit(&restore)
  # Ctrl-C is handled explicitly as a byte in InputHandler because raw mode disables the terminal's own SIGINT generation; these traps are only a backstop for termination from outside (e.g. `kill`).
  Signal.trap("INT") { restore.call; exit(0) }
  Signal.trap("TERM") { restore.call; exit(0) }

  $stdout.write(ENTER_ALT_SCREEN)
  $stdout.flush
  begin
    $stdin.raw do
      status_window_start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
      status_ticks = 0
      status_text = "dewasm DOOM | starting... | q/^C quit  f fire  space use  arrows move  ,/. strafe  tab automap"
      loop do
        now = Process.clock_gettime(Process::CLOCK_MONOTONIC)
        input.poll(now)
        break if input.quit?

        doom.invoke("tickGame")
        status_ticks += 1
        elapsed = now - status_window_start
        if elapsed >= 0.5
          rate = status_ticks / elapsed
          status_text = format(
            "dewasm DOOM | %.1f ticks/sec | q/^C quit  f fire  space use  arrows move  ,/. strafe  tab automap", rate
          )
          status_ticks = 0
          status_window_start = now
        end

        $stdout.write(renderer.render(frame_state[:pixels], frame_state[:width], frame_state[:height], status_text))
      end
    end
  ensure
    restore.call
  end
end

check_yjit!
if ARGV.include?("--smoke")
  run_smoke
else
  run_interactive
end
