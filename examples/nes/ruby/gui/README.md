# NES (Ruby, gosu window)

An interactive NES frontend that renders into a window with [gosu](https://www.libgosu.org/).
gosu is the SDL2-backed 2D game library for Ruby.
The parent [`../`](../) frontend draws the same emulator into a terminal; this one takes a real window.
It is the NES counterpart of [`../../../doom/ruby/gui`](../../../doom/ruby/gui), in the same shape.
The generated library is shared with the terminal frontend.
`../build.sh` regenerates `../nes_gen.rb`, which both require.
gosu is scoped to this directory with bundler.

## Requirements

`run.sh` installs gosu with bundler, into the gitignored `vendor/bundle` of this directory.
The version is pinned by `Gemfile.lock`.
Nothing is installed globally, and the parent terminal frontend stays stdlib-only.
The gem builds a native extension against SDL2.
The development-header packages are in the [DOOM gui frontend's README](../../../doom/ruby/gui/README.md#requirements).
The current macOS sdl2-compat caveat is there too.

## Run

```sh
./run.sh
./run.sh path/to/game.nes   # any ROM agnes's mappers cover
```

builds and opens a window.
The window is resizable.
The frame keeps its ratio and is centered.
Black bars appear on whichever axis runs out first.

| Option | Effect |
| --- | --- |
| `--scale N` | Window size as a multiple of 256x240, 1 to 8 (default 3, so 768x720). |
| `--fullscreen` | Start fullscreen. |
| `--smooth` | Scale the frame with interpolation instead of nearest-neighbor. |
| `--smoke` | Headless self-check, described below. |

`./run.sh --smoke` needs no display.
It inits the emulator and ticks it 300 times with no input.
It reports the tick rate and the per-frame conversion cost.
It sanity-checks the last frame, writes it to `screenshot.png`, and exits non-zero on failure.

## Rendering

The module hands over agnes's own frame representation.
That is one palette *index* per pixel at `screenOffset()`.
The indices resolve against the fixed 64-entry palette at `paletteOffset()`.
That keeps the per-pixel conversion to one Array lookup.
Each of the 256 possible index bytes maps to a precomputed 32-bit RGBA word.
The table folds in the module's `& 0x3f` mask.
So a frame is one `unpack`, one `map!` over the table and one `pack`.
All of it runs at C level except the lookups.
The 256x240 result is uploaded once per frame, and the GPU does the scaling.
So the render cost does not grow with the window.
Nearest-neighbor versus interpolated scaling works exactly as in the DOOM gui frontend.
That is a `retro: true` render target, with `--smooth` to opt out.

Pacing is gosu's own 60Hz update interval.
The NTSC NES's real rate is ~60.0988Hz, close enough that no calibration is needed.
When the interpreter cannot sustain 60 ticks/sec, updates simply run late.
That is the same fastest-sustainable-rate behavior the terminal frontend implements by hand.

## Controls

The terminal frontend has to synthesize key releases after a hold window.
gosu instead reports real `button_down`/`button_up` events.
The held-button bitmask `setInput` wants every tick falls out of them directly.

| Key | Action |
| --- | --- |
| Arrow keys | D-pad |
| x | A |
| z | B |
| Enter | Start |
| Space | Select |
| F1 | Show or hide the on-screen status bar |
| q / Escape | Quit |

The bundled ROM is [Alter Ego](https://forums.nesdev.org/viewtopic.php?t=7999) by Shiru, released into the public domain.
