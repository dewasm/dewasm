#!/usr/bin/env ruby
# frozen_string_literal: true

# Interactive windowed frontend for the dewasm-generated NES library, rendering with Gosu.
# The library is `../nes_gen.rb`, shared with the terminal frontend.
# `../build.sh` produces it from `cache/nes.wasm`, which is built on `agnes`.
#
# The parent `../main.rb` draws into a terminal; this one takes a real window.
# Like there, `nes.wasm` has zero host imports, so the host drives everything itself.
# That covers pacing, input polling, and frame presentation.
# Pacing is Gosu's 60Hz update interval, close enough to the NTSC NES's ~60.0988Hz.
# What the window buys over the terminal is real key releases.
# `setInput` wants the full held-button bit mask every tick.
# Gosu's `button_down`/`button_up` report exactly that.
# A terminal has to generate releases after a hold window instead.
#
# Run with `--smoke` for a headless self-check (no window, no display needed).

require_relative "../nes_gen"
require "gosu"

DEFAULT_ROM_PATH = File.join(__dir__, "..", "..", "..", "apps", "cache", "alter_ego.nes")

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

def new_nes
  nes = Nes.new
  # Reactor initialization comes before any other export.
  # `nes.wasm` has no WASI surface to run it implicitly.
  nes.invoke("_initialize")
  nes
end

# Turns the module's frame into the RGBA blob Gosu wants.
# The frame is one palette index per pixel at `screenOffset`.
# The indices refer to the fixed 64-entry palette at `paletteOffset`.
#
# The guest hands over indices, not colors, which keeps the per-pixel work to one Array lookup.
# Each of the 256 possible index bytes maps to a 32-bit RGBA word computed ahead of time.
# The module's `& 0x3f` mask is folded into that table.
# So a frame is one `unpack`, one `map!` over the lookup table, and one `pack`.
# All of it runs at C level except the lookups.
class FrameConverter
  def initialize(palette)
    # `pack("V*")` writes little-endian.
    # So the word R | G<<8 | B<<16 | A<<24 lands in memory as the R,G,B,A bytes Gosu reads.
    @lut = Array.new(256) do |i|
      r, g, b = palette[i & 0x3f]
      r | (g << 8) | (b << 16) | (0xff << 24)
    end
  end

  def convert(screen)
    screen.unpack("C*").map! { |ix| @lut[ix] }.pack("V*")
  end
end

# The module's fixed 64-entry palette (R,G,B,A per entry; alpha is padding).
# It is read once: it never changes, unlike the per-frame index buffer.
def read_palette(nes)
  bytes = nes.memory.buffer.get_string(nes.invoke("paletteOffset"), 64 * 4).bytes
  Array.new(64) { |i| bytes[i * 4, 3] }
end

CONTROLS_TEXT = "arrows d-pad   x=A z=B   enter start   space select   F1 hud   q/esc quit"

class NesWindow < Gosu::Window
  # The NTSC NES runs at ~60.0988Hz.
  # 60 exactly is close enough that no separate calibration is needed.
  # When the interpreter cannot sustain it, Gosu's update runs late.
  # That is the same fastest-sustainable-rate behavior the terminal frontend implements by hand.
  TICKS_PER_SECOND = 60
  # The `caption` only has to be readable, not frame-accurate.
  # Asking the window manager to retitle the window is not free.
  CAPTION_UPDATE_EVERY = TICKS_PER_SECOND
  HUD_HEIGHT = 18

  def initialize(nes, frame_w, frame_h, converter, screen_off, scale:, fullscreen:, retro:)
    super(frame_w * scale, frame_h * scale, fullscreen: fullscreen, resizable: true)
    self.caption = "NES (dewasm)"
    self.update_interval = 1000.0 / TICKS_PER_SECOND

    @frame_w = frame_w
    @frame_h = frame_h
    @converter = converter
    @screen_off = screen_off
    # Fetched once out of the exports table.
    # `invoke()` would repeat the hash lookup and splat the arguments on every one of these calls.
    @set_input = nes.exports.fetch("setInput")
    @tick_game = nes.exports.fetch("tickGame")
    @buffer = nes.memory.buffer
    @held = 0
    @rgba = nil

    @retro = retro
    @canvas = nil
    @font = Gosu::Font.new(13)
    @show_hud = true
    @ticks = 0
    @rate_ticks = 0
    @rate_window_start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    @rate = 0.0
  end

  def update
    @set_input.call(@held)
    @tick_game.call
    @rgba = @converter.convert(@buffer.get_string(@screen_off, @frame_w * @frame_h))
    @ticks += 1
    @rate_ticks += 1
    return unless (@ticks % CAPTION_UPDATE_EVERY).zero?

    now = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    @rate = @rate_ticks / (now - @rate_window_start)
    @rate_ticks = 0
    @rate_window_start = now
    self.caption = format("NES (dewasm) - %.1f ticks/sec", @rate)
  end

  def draw
    rgba = @rgba
    return unless rgba

    image = frame_image(rgba)
    # The window is resizable and need not keep the frame's ratio.
    # So the frame is scaled by whichever axis runs out first, and centred.
    # That leaves black bars on the other axis.
    scale = [width.to_f / image.width, height.to_f / image.height].min
    draw_w = image.width * scale
    draw_h = image.height * scale
    image.draw((width - draw_w) / 2, (height - draw_h) / 2, 0, scale, scale)
    draw_hud if @show_hud
  end

  def button_down(id)
    case id
    when Gosu::KB_ESCAPE, Gosu::KB_Q, Gosu::KB_F10 then close
    when Gosu::KB_F1 then @show_hud = !@show_hud
    else
      bit = button_bit(id)
      @held |= bit if bit
    end
  end

  def button_up(id)
    bit = button_bit(id)
    @held &= ~bit if bit
  end

  private

  def button_bit(id)
    case id
    when Gosu::KB_UP then BUTTON_BITS[:up]
    when Gosu::KB_DOWN then BUTTON_BITS[:down]
    when Gosu::KB_LEFT then BUTTON_BITS[:left]
    when Gosu::KB_RIGHT then BUTTON_BITS[:right]
    when Gosu::KB_X then BUTTON_BITS[:a]
    when Gosu::KB_Z then BUTTON_BITS[:b]
    when Gosu::KB_RETURN, Gosu::KB_ENTER then BUTTON_BITS[:start]
    when Gosu::KB_SPACE then BUTTON_BITS[:select]
    end
  end

  # Sends the frame to the GPU.
  # Gosu interpolates a scaled-up image unless the texture was created with `retro: true`.
  # `Image.from_blob` has no way to ask for that.
  # So the `retro` path draws the frame into a nearest-neighbor render target kept across frames.
  # That is what `--smooth` turns off.
  def frame_image(rgba)
    image = Gosu::Image.from_blob(@frame_w, @frame_h, rgba)
    return image unless @retro

    @canvas ||= Gosu.render(image.width, image.height, retro: true) { }
    @canvas.insert(image, 0, 0)
    @canvas
  end

  # A dark bar along the bottom edge.
  # It keeps the tick rate and the controls readable against the game's own palette.
  def draw_hud
    bar_y = height - HUD_HEIGHT
    draw_rect(0, bar_y, width, HUD_HEIGHT, Gosu::Color.new(180, 0, 0, 0), 1)
    @font.draw_text(format("%.1f ticks/sec  |  %s", @rate, CONTROLS_TEXT), 5, bar_y + 3, 2)
  end
end

def check_yjit!
  return if defined?(RubyVM::YJIT) && RubyVM::YJIT.enabled?

  warn "nes: YJIT is not enabled (run with `ruby --yjit`, or set RUBY_YJIT_ENABLE=1) " \
       "- the Ruby backend is already dewasm's slowest, and needs YJIT to stay playable."
end

def start_nes(rom_path)
  nes = new_nes
  load_rom(nes, rom_path)
  frame_w = nes.invoke("frameWidth")
  frame_h = nes.invoke("frameHeight")
  screen_off = nes.invoke("screenOffset")
  converter = FrameConverter.new(read_palette(nes))
  [nes, frame_w, frame_h, converter, screen_off]
end

def run_smoke(rom_path)
  nes, frame_w, frame_h, converter, screen_off = start_nes(rom_path)
  set_input = nes.exports.fetch("setInput")
  tick_game = nes.exports.fetch("tickGame")

  ticks = 300
  start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  ticks.times do
    set_input.call(0)
    tick_game.call
  end
  elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - start

  # Timed separately from the loop above.
  # The interactive window converts once per tick.
  # But the tick cost is what varies between machines and Rubies.
  screen = nes.memory.buffer.get_string(screen_off, frame_w * frame_h)
  convert_start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  rgba = nil
  10.times { rgba = converter.convert(screen) }
  convert_ms = (Process.clock_gettime(Process::CLOCK_MONOTONIC) - convert_start) / 10 * 1000

  puts format(
    "smoke: ran %d ticks in %.3fs - %.1f ticks/sec (%.2fms/frame framebuffer conversion)",
    ticks, elapsed, ticks / elapsed, convert_ms
  )

  distinct = rgba.unpack("V*").uniq.size
  puts "smoke: final frame is #{frame_w}x#{frame_h} with #{distinct} distinct colors"
  # The NES PPU palette tops out at 64 colors total, so a healthy frame lands in the dozens.
  # A degenerate (blank/solid) frame lands in the single digits.
  # It is the >4 threshold of the snapshot oracle (`crates/xtask-snapshot/src/nes_snapshot.rs`).
  if distinct <= 4
    warn "smoke: FAIL: frame looks degenerate (too few distinct colors)"
    exit 1
  end

  # Gosu decodes and encodes images without a window.
  # So this writes a real PNG with no display attached.
  # The terminal frontend writes PPM instead, since Ruby's standard library has no PNG writer.
  Gosu::Image.from_blob(frame_w, frame_h, rgba).save("screenshot.png")
  puts "smoke: wrote #{File.expand_path('screenshot.png')}"
end

def parse_options(argv)
  options = { scale: 3, fullscreen: false, retro: true }
  rom_path = nil
  until argv.empty?
    case (arg = argv.shift)
    when "--scale" then options[:scale] = Integer(argv.shift, exception: false) || 0
    when "--fullscreen" then options[:fullscreen] = true
    when "--smooth" then options[:retro] = false
    else
      if arg.start_with?("--") || rom_path
        warn "nes: unknown option #{arg}"
        warn "usage: main.rb [--smoke] [--scale N] [--fullscreen] [--smooth] [rom.nes]"
        exit 2
      end
      rom_path = arg
    end
  end
  unless (1..8).cover?(options[:scale])
    warn "nes: --scale must be an integer between 1 and 8"
    exit 2
  end
  [options, rom_path || DEFAULT_ROM_PATH]
end

def run_interactive(options, rom_path)
  # `Gosu.render`, which the nearest-neighbor path needs, arrived in Gosu 1.1.
  # Before that, degrade to interpolated scaling rather than refusing to run.
  if options[:retro] && !Gosu.respond_to?(:render)
    warn "nes: this gosu is too old for nearest-neighbor scaling; falling back to --smooth"
    options[:retro] = false
  end
  nes, frame_w, frame_h, converter, screen_off = start_nes(rom_path)
  NesWindow.new(nes, frame_w, frame_h, converter, screen_off, **options).show
end

check_yjit!
argv = ARGV.dup
smoke = argv.delete("--smoke") ? true : false
options, rom_path = parse_options(argv)
if smoke
  run_smoke(rom_path)
else
  run_interactive(options, rom_path)
end
