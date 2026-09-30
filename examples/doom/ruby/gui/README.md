# DOOM (Ruby, gosu window)

An interactive DOOM frontend that renders into a window with [gosu](https://www.libgosu.org/).
gosu is the SDL2-backed 2D game library for Ruby.
The parent [`../`](../) frontend draws the same game into a terminal; this one takes a real window.

Both frontends share one generated library.
`../build.sh` fetches jacobenget/doom.wasm, checksum-pinned into the shared apps cache.
It converts the module to Ruby with dewasm, into `../doom_gen.rb`.
That file is ~11MB, gitignored, and regenerated on every build.
`main.rb` implements the module's ten host imports.
They cover console logging, save-game I/O, the game clock, and frame delivery.
`main.rb` also has the gosu window, its renderer and its keyboard input.

## Requirements

`run.sh` installs gosu with bundler, into the gitignored `vendor/bundle` of this directory.
The version is pinned by `Gemfile.lock`.
Nothing is installed globally, and the parent terminal frontend stays stdlib-only.

The gem builds a native extension against SDL2, so its development headers have to be present:

| Platform | Install |
| --- | --- |
| Debian/Ubuntu | `sudo apt install libsdl2-dev` |
| Fedora | `sudo dnf install SDL2-devel` |
| macOS | `brew install sdl2` |

On macOS, Homebrew's `sdl2` currently resolves to sdl2-compat.
Its `sdl2-config` does not support the `--static-libs` flag gosu's build uses.
The extension then builds without its SDL2 and AppKit link line.
It fails at require time with a missing-symbol error.
Until gosu handles sdl2-compat, use one of two workarounds:

- Install against a real SDL2 (`brew install sdl2 --formula` with a pre-compat bottle, or MacPorts).
- Wrap `sdl2-config` so `--static-libs` answers with the `--libs` output.

## Why a window is affordable here

The terminal frontend exists because the Ruby backend only manages ~15 ticks/sec under YJIT.
That is far below what a GUI usually needs.
A terminal has orders of magnitude fewer cells to redraw than a window has pixels.
Here a window costs no more than a terminal: the per-frame work does not scale with the window.
The frame is uploaded once as a 320x200 texture, and the GPU does the scaling.
So `--scale 6` costs exactly what `--scale 1` costs.

The module hands over a 640x400 framebuffer, but DOOM renders at 320x200 and upscales by exactly 2.
Its own startup log reads `I_InitGraphics: Auto-scaling factor: 2`.
Halving it back is therefore lossless.
It leaves 64000 pixels per frame to convert instead of 256000.
`FrameConverter` verifies that on the first frame rather than trusting it.
It falls back to full resolution if a future build of the module ever stops holding it.

What the window buys over the terminal is real key releases.
Terminals report presses only.
So the terminal frontend must synthesize a release after a hold window.
It also cannot deliver Ctrl as a plain key at all.
Here Ctrl (fire) and Shift (run) work as they do in DOOM.

## Run

```sh
./run.sh
```

builds and opens a window.
The window is resizable.
The frame keeps its 8:5 aspect ratio and is centered.
Black bars appear on whichever axis runs out first.

| Option | Effect |
| --- | --- |
| `--scale N` | Window size as a multiple of 320x200, 1 to 8 (default 3, so 960x600). |
| `--fullscreen` | Start fullscreen. |
| `--smooth` | Scale the frame with interpolation instead of nearest-neighbor, saving ~10ms per frame. |
| `--smoke` | Headless self-check, described below. |

`./run.sh --smoke` needs no display.
It inits the game and ticks it 60 times with no window.
It reports the tick rate and the per-frame conversion cost.
It sanity-checks the last frame, writes it to `screenshot.png`, and exits non-zero on failure.
gosu encodes PNGs without a graphics context, so this frontend writes a real PNG.
The terminal frontend instead falls back to binary PPM, for want of a stdlib PNG writer.

## Rendering

Handing a frame to gosu takes two per-pixel transformations.
Written as a straightforward Ruby loop, they would cost more than the wasm tick itself.
No other part of this frontend has that problem.
The module stores pixels as B,G,R,A while gosu wants R,G,B,A.
The module's alpha byte is always 0, where gosu needs 0xff.

Two facts about DOOM's renderer keep that to a few hundred Ruby-level operations per frame.
The count would otherwise be 64000.
It is paletted (VGA Mode 13h), with at most 256 colors per palette.
A session uses a handful of palettes.
So each distinct 32-bit source word is swizzled once and memoized.
A real frame holds around 175 of them.
Its 640x400 output is an exact 2x upscale.
So each row is read with a single `unpack("Vx4" * 320)`.
There `V` takes a 32-bit pixel, and `x4` skips the duplicated neighbor.
That leaves one `unpack`, one memoized `map!` and one `pack` per row.
All of it runs at C level except the hash lookups.

Uploading the result is one `Gosu::Image.from_blob` per frame.
gosu interpolates an image scaled past its native size.
The exception is an image whose texture was created with `retro: true`.
`Image.from_blob` has no parameter for that.
So the default path blits the frame into a persistent nearest-neighbor render target instead.
That is what `--smooth` turns off, trading DOOM's crisp pixels for the ~10ms.

Measured on an x86-64 Linux container under Ruby 3.3.6 **without** YJIT, at the default `--scale 3`.
That build of Ruby has no YJIT support.

| Step | Cost per frame |
| --- | --- |
| Framebuffer conversion (640x400 BGRA to 320x200 RGBA) | 9.9ms |
| `Gosu::Image.from_blob` | 2.0ms |
| Nearest-neighbor blit (skipped by `--smooth`) | 10.0ms |
| One `tickGame` | 226ms |

The wasm tick dominates by an order of magnitude, which is the point.
Rendering is not what makes this frontend slow.
YJIT is what moves the tick figure.
`run.sh` always passes it, and `main.rb` warns on stderr if it ends up missing.
The terminal frontend measures the same Ruby backend at 15.9 ticks/sec with YJIT.
That figure is from an Apple Silicon laptop.

## Controls

| Key | Action |
| --- | --- |
| Arrow keys | Move / turn |
| Ctrl | Fire |
| Space | Use (open doors, flip switches) |
| Shift | Run |
| , / . | Strafe left / right |
| Tab | Automap |
| Enter | Menu confirm |
| Escape | Menu / pause |
| Backspace | Menu back |
| 0-9 | Weapon select / text entry |
| Letters | Text entry, `y` / `n` prompts |
| F1 | Show or hide the on-screen status bar |
| F10 | Quit |

gosu closes a window on Escape unless the frontend overrides its `button_down`.
This frontend does override it.
Escape belongs to DOOM's menu, and quitting is F10 or the window's close button.

Save games are written to `.savegame/` (gitignored) relative to wherever the script runs.

No sound: the module exposes no audio interface.
