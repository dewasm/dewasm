#!/usr/bin/env ruby
# frozen_string_literal: true

# Interactive windowed frontend for the dewasm-generated DOOM library, rendering with gosu.
# The library is ../doom_gen.rb, shared with the terminal frontend.
# ../build.sh produces it from jacobenget/doom.wasm.
#
# The parent ../main.rb draws into a terminal.
# The Ruby backend only manages ~15 ticks/sec under YJIT.
# And a terminal has orders of magnitude fewer cells than a window has pixels.
# This one takes the window anyway.
# That is affordable because the module's 640x400 framebuffer is an exact 2x upscale of 320x200.
# That is DOOM's native resolution.
# Halving it back is lossless and leaves 64000 pixels per frame to hand to the GPU, not 256000.
# What the window buys over the terminal is real key releases.
# So Ctrl (fire) and Shift (run) work as they do in DOOM.
#
# Run with --smoke for a headless self-check (no window, no display needed).

require_relative "../doom_gen"
require "gosu"

SAVE_DIR = ".savegame"

def save_game_path(id)
  File.join(SAVE_DIR, "doomsav#{id}.dsg")
end

# Connects the wasm module's ten host imports to Ruby.
# `doom_holder` exists because these closures have to be built before Doom.new returns the instance.
# The closures read memory from that instance.
# `doom_holder` is filled in immediately after construction.
# It is only read from within calls the imports themselves receive later, never during Doom.new.
def build_imports(doom_holder, frame_state)
  {
    "console" => {
      "onErrorMessage" => lambda do |off, len|
        warn doom_holder[0].memory.buffer.get_string(off, len)
      end,
      "onInfoMessage" => lambda do |off, len|
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
      # Backs DOOM's internal 35Hz pacing.
      # So it has to be a real monotonic clock, not a fake stepped one.
      # Otherwise the game's notion of elapsed time would drift from how often we call tickGame.
      "timeInMilliseconds" => lambda do
        Process.clock_gettime(Process::CLOCK_MONOTONIC, :millisecond)
      end,
    },
    "ui" => {
      # Converted here, inside the tick, rather than copied out for later.
      # A subsequent tick that grows the module's memory can replace the memory buffer this reads.
      "drawFrame" => lambda do |buf_off|
        frame_state[:buf_off] = buf_off
        frame_state[:rgba] = frame_state[:converter].convert(doom_holder[0].memory.buffer, buf_off)
      end,
    },
    "loading" => {
      "onGameInit" => lambda do |w, h|
        frame_state[:width] = w
        frame_state[:height] = h
        frame_state[:converter] = FrameConverter.new(w, h)
      end,
      # Leaving both output slots untouched (they arrive pre-zeroed) selects the embedded WAD.
      # That is the wasm-embedded shareware WAD.
      # Supplying external WADs is out of scope for this frontend.
      "wadSizes" => lambda { |_num_off, _bytes_off| },
      "readWads" => lambda { |_dst_off, _lengths_off| },
    },
  }
end

# Turns the module's framebuffer into the RGBA blob gosu wants.
#
# There are two transformations, both per pixel.
# So only here in this frontend would a naive Ruby loop cost more than the wasm tick itself.
# First, the module stores B,G,R,A while gosu wants R,G,B,A.
# Second, the module's alpha byte is always 0 where gosu needs 0xff.
#
# Two facts about DOOM's renderer keep the work to a few hundred Ruby-level operations per frame.
# Without them it would be 64000.
# First, the renderer is paletted: VGA Mode 13h, at most 256 colors per palette.
# A session has only a handful of palettes.
# So every distinct 32-bit source word is swizzled once and memoized.
# Second, its 640x400 output is an exact 2x nearest upscale of its native 320x200.
# So reading every other pixel of every other row is lossless.
# The first frame checks that rather than trusting it.
# It falls back to full resolution if a future build of the module ever stops holding it.
class FrameConverter
  attr_reader :out_w, :out_h

  # out_w/out_h are not known until the first frame decides whether halving is lossless.
  # So they start at full resolution, and #convert narrows them once.
  def initialize(src_w, src_h)
    @src_w = src_w
    @src_h = src_h
    @src_stride = src_w * 4
    @step = nil
    @out_w = src_w
    @out_h = src_h
    @swizzled = Hash.new do |memo, word|
      memo[word] = 0xff000000 | ((word & 0x0000ff) << 16) | (word & 0x00ff00) | ((word >> 16) & 0xff)
    end
  end

  def convert(buffer, base_off)
    choose_step(buffer, base_off) unless @step
    rgba = String.new(capacity: @out_w * @out_h * 4)
    @out_h.times do |y|
      row = buffer.get_string(base_off + y * @step * @src_stride, @src_stride)
      rgba << row.unpack(@row_template).map! { |word| @swizzled[word] }.pack("V*")
    end
    rgba
  end

  private

  def choose_step(buffer, base_off)
    @step = upscaled_2x?(buffer, base_off) ? 2 : 1
    warn "doom: framebuffer is not a 2x upscale; rendering at full #{@src_w}x#{@src_h}" if @step == 1
    @out_w = @src_w / @step
    @out_h = @src_h / @step
    # One C-level unpack per row.
    # "V" reads a 32-bit BGRA word, and "x4" skips the duplicated neighbor.
    @row_template = ((@step == 2 ? "Vx4" : "V") * @out_w).freeze
  end

  # True when every 2x2 block of the framebuffer is uniform.
  # That is, the image carries no more detail than its 320x200 half.
  def upscaled_2x?(buffer, base_off)
    return false unless @src_w.even? && @src_h.even?

    even_pixels = ("Vx4" * (@src_w / 2)).freeze
    odd_pixels = ("x4V" * (@src_w / 2)).freeze
    (@src_h / 2).times do |y|
      top = buffer.get_string(base_off + y * 2 * @src_stride, @src_stride)
      return false unless top == buffer.get_string(base_off + (y * 2 + 1) * @src_stride, @src_stride)
      return false unless top.unpack(even_pixels) == top.unpack(odd_pixels)
    end
    true
  end
end

# Translates a gosu button id to the code reportKeyDown/reportKeyUp expect.
# For special keys that is the module's KEY_* constant.
# For letters and digits it is the ASCII value of the lowercase character.
# Those serve text entry, and weapon-select digits.
class KeyMap
  def initialize(doom)
    @by_button = {
      Gosu::KB_LEFT => doom.global_get("KEY_LEFTARROW"),
      Gosu::KB_RIGHT => doom.global_get("KEY_RIGHTARROW"),
      Gosu::KB_UP => doom.global_get("KEY_UPARROW"),
      Gosu::KB_DOWN => doom.global_get("KEY_DOWNARROW"),
      Gosu::KB_LEFT_CONTROL => doom.global_get("KEY_FIRE"),
      Gosu::KB_RIGHT_CONTROL => doom.global_get("KEY_FIRE"),
      Gosu::KB_SPACE => doom.global_get("KEY_USE"),
      Gosu::KB_LEFT_SHIFT => doom.global_get("KEY_SHIFT"),
      Gosu::KB_RIGHT_SHIFT => doom.global_get("KEY_SHIFT"),
      Gosu::KB_LEFT_ALT => doom.global_get("KEY_ALT"),
      Gosu::KB_RIGHT_ALT => doom.global_get("KEY_ALT"),
      Gosu::KB_TAB => doom.global_get("KEY_TAB"),
      Gosu::KB_ESCAPE => doom.global_get("KEY_ESCAPE"),
      Gosu::KB_RETURN => doom.global_get("KEY_ENTER"),
      Gosu::KB_ENTER => doom.global_get("KEY_ENTER"),
      Gosu::KB_BACKSPACE => doom.global_get("KEY_BACKSPACE"),
      Gosu::KB_COMMA => doom.global_get("KEY_STRAFE_L"),
      Gosu::KB_PERIOD => doom.global_get("KEY_STRAFE_R"),
    }
    (Gosu::KB_A..Gosu::KB_Z).each_with_index { |id, i| @by_button[id] = "a".ord + i }
    # KB_1..KB_9 run 30..38 and KB_0 sits above them at 39.
    # So the digit row is not one contiguous ascending range.
    (Gosu::KB_1..Gosu::KB_9).each_with_index { |id, i| @by_button[id] = "1".ord + i }
    @by_button[Gosu::KB_0] = "0".ord
  end

  def [](button_id) = @by_button[button_id]
end

CONTROLS_TEXT = "arrows move   ctrl fire   space use   shift run   ,/. strafe   tab automap   " \
                "esc menu   1-7 weapon   F1 hud   F10 quit"

class DoomWindow < Gosu::Window
  # DOOM's own internal tic rate.
  # tickGame paces itself off runtimeControl.timeInMilliseconds no matter how often it is called.
  # So matching 35Hz makes one update advance exactly one tic.
  # Otherwise some updates would be no-ops.
  TICKS_PER_SECOND = 35
  # The caption only has to be legible, not frame-accurate.
  # Asking the window manager to retitle the window is not free.
  CAPTION_UPDATE_EVERY = TICKS_PER_SECOND
  HUD_HEIGHT = 18

  def initialize(doom, frame_state, scale:, fullscreen:, retro:)
    @converter = frame_state.fetch(:converter)
    super(@converter.out_w * scale, @converter.out_h * scale, fullscreen: fullscreen, resizable: true)
    self.caption = "DOOM (dewasm)"
    self.update_interval = 1000.0 / TICKS_PER_SECOND

    @frame_state = frame_state
    @keys = KeyMap.new(doom)
    # Fetched once out of the exports table.
    # invoke() would redo the hash lookup and splat the arguments on every one of these calls.
    @tick_game = doom.exports.fetch("tickGame")
    @report_key_down = doom.exports.fetch("reportKeyDown")
    @report_key_up = doom.exports.fetch("reportKeyUp")

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
    @tick_game.call # drives ui.drawFrame internally, refreshing frame_state[:rgba]
    @ticks += 1
    @rate_ticks += 1
    return unless (@ticks % CAPTION_UPDATE_EVERY).zero?

    now = Process.clock_gettime(Process::CLOCK_MONOTONIC)
    @rate = @rate_ticks / (now - @rate_window_start)
    @rate_ticks = 0
    @rate_window_start = now
    self.caption = format("DOOM (dewasm) - %.1f ticks/sec", @rate)
  end

  def draw
    rgba = @frame_state[:rgba]
    return unless rgba

    image = frame_image(rgba)
    # Letterbox: the window is resizable and need not keep the framebuffer's 8:5 ratio.
    # So the frame is scaled by whichever axis runs out first, and centred.
    # That leaves black bars on the other axis.
    scale = [width.to_f / image.width, height.to_f / image.height].min
    draw_w = image.width * scale
    draw_h = image.height * scale
    image.draw((width - draw_w) / 2, (height - draw_h) / 2, 0, scale, scale)
    draw_hud if @show_hud
  end

  def button_down(id)
    # Deliberately no `super`: gosu's default button_down closes the window on Escape.
    # Escape is DOOM's menu key.
    case id
    when Gosu::KB_F10 then close
    when Gosu::KB_F1 then @show_hud = !@show_hud
    else
      code = @keys[id]
      @report_key_down.call(code) if code
    end
  end

  def button_up(id)
    code = @keys[id]
    @report_key_up.call(code) if code
  end

  private

  # Uploads the frame to the GPU.
  # Gosu interpolates a scaled-up image unless the texture was created with `retro: true`.
  # Image.from_blob has no way to ask for that.
  # So the retro path blits the frame into a persistent nearest-neighbor render target instead.
  # That costs about 10ms a frame against from_blob's 2, which is why --smooth exists.
  def frame_image(rgba)
    image = Gosu::Image.from_blob(@converter.out_w, @converter.out_h, rgba)
    return image unless @retro

    @canvas ||= Gosu.render(image.width, image.height, retro: true) { }
    @canvas.insert(image, 0, 0)
    @canvas
  end

  # A dark bar along the bottom edge.
  # It keeps the tick rate and the controls legible against DOOM's own (highly variable) palette.
  def draw_hud
    bar_y = height - HUD_HEIGHT
    draw_rect(0, bar_y, width, HUD_HEIGHT, Gosu::Color.new(180, 0, 0, 0), 1)
    @font.draw_text(format("%.1f ticks/sec  |  %s", @rate, CONTROLS_TEXT), 5, bar_y + 3, 2)
  end
end

def check_yjit!
  return if defined?(RubyVM::YJIT) && RubyVM::YJIT.enabled?

  warn "doom: YJIT is not enabled (run with `ruby --yjit`, or set RUBY_YJIT_ENABLE=1) " \
       "- the Ruby backend is already dewasm's slowest, and needs YJIT to stay playable."
end

def start_doom
  frame_state = { width: 0, height: 0, rgba: nil, buf_off: nil, converter: nil }
  doom_holder = [nil]
  doom = Doom.new(build_imports(doom_holder, frame_state))
  doom_holder[0] = doom
  doom.invoke("initGame")
  # initGame renders the title screen.
  # So a frame has been through FrameConverter by now, and out_w/out_h have settled.
  # The window sizes itself from them.
  # It would pick the unhalved 640x400 if this were ever not true.
  if frame_state[:converter].nil? || frame_state[:rgba].nil?
    warn "doom: FAIL: initGame produced no frame (loading.onGameInit or ui.drawFrame was never called)"
    exit 1
  end
  [doom, frame_state]
end

def run_smoke
  doom, frame_state = start_doom
  converter = frame_state.fetch(:converter)
  tick_game = doom.exports.fetch("tickGame")

  ticks = 60
  start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  ticks.times { tick_game.call }
  elapsed = Process.clock_gettime(Process::CLOCK_MONOTONIC) - start

  rgba = frame_state[:rgba]
  unless rgba
    warn "smoke: FAIL: no frame was ever captured"
    exit 1
  end

  # Timed separately from the loop above.
  # Conversion runs inside the tick, as part of ui.drawFrame.
  convert_start = Process.clock_gettime(Process::CLOCK_MONOTONIC)
  10.times { converter.convert(doom.memory.buffer, frame_state.fetch(:buf_off)) }
  convert_ms = (Process.clock_gettime(Process::CLOCK_MONOTONIC) - convert_start) / 10 * 1000

  puts format(
    "smoke: ran %d ticks in %.3fs - %.1f ticks/sec (%.2fms/frame framebuffer conversion)",
    ticks, elapsed, ticks / elapsed, convert_ms
  )

  distinct = rgba.unpack("V*").uniq.size
  puts "smoke: final frame is #{converter.out_w}x#{converter.out_h} with #{distinct} distinct colors"
  # DOOM's software renderer is paletted (classic VGA Mode 13h: at most 256 colors).
  # So a healthy frame tops out in the low hundreds.
  # A truecolor renderer would produce thousands.
  # A degenerate frame (blank/solid) instead lands in the single digits.
  if distinct <= 50
    warn "smoke: FAIL: frame looks degenerate (too few distinct colors)"
    exit 1
  end

  # Gosu decodes and encodes images without a window.
  # So this writes a real PNG with no display attached.
  # The terminal frontend falls back to PPM instead, for want of a stdlib PNG writer.
  Gosu::Image.from_blob(converter.out_w, converter.out_h, rgba).save("screenshot.png")
  puts "smoke: wrote #{File.expand_path('screenshot.png')}"
end

def parse_options(argv)
  options = { scale: 3, fullscreen: false, retro: true }
  until argv.empty?
    case (arg = argv.shift)
    when "--scale" then options[:scale] = Integer(argv.shift, exception: false) || 0
    when "--fullscreen" then options[:fullscreen] = true
    when "--smooth" then options[:retro] = false
    else
      warn "doom: unknown option #{arg}"
      warn "usage: main.rb [--smoke] [--scale N] [--fullscreen] [--smooth]"
      exit 2
    end
  end
  unless (1..8).cover?(options[:scale])
    warn "doom: --scale must be an integer between 1 and 8"
    exit 2
  end
  options
end

def run_interactive(options)
  # Gosu.render, which the nearest-neighbor path needs, arrived in gosu 1.1.
  # Before that, degrade to interpolated scaling rather than refusing to run.
  if options[:retro] && !Gosu.respond_to?(:render)
    warn "doom: this gosu is too old for nearest-neighbor scaling; falling back to --smooth"
    options[:retro] = false
  end
  doom, frame_state = start_doom
  DoomWindow.new(doom, frame_state, **options).show
end

check_yjit!
if ARGV.delete("--smoke")
  run_smoke
else
  run_interactive(parse_options(ARGV))
end
