# DOOM (Ruby, Gosu window)

An interactive DOOM frontend that renders into a window with [Gosu](https://www.libgosu.org/).
Gosu is the SDL2-backed 2D game library for Ruby.
The parent [`../`](../) frontend draws the same game into a terminal; this one takes a real window.

Both frontends share one generated library.
`../build.sh` fetches `jacobenget/doom.wasm`, fixed by checksum, into the shared apps cache.
It converts the module to Ruby with dewasm, into `../doom_gen.rb`.
That file is ~11MB, ignored by Git, and regenerated on every build.
`main.rb` implements the module's ten host imports.
They cover console logging, save-game I/O, the game clock, and frame delivery.
`main.rb` also has the Gosu window, its renderer and its key input.

## Requirements

`run.sh` installs the `gosu` gem with Bundler into this directory's `vendor/bundle`, ignored by Git.
`Gemfile.lock` fixes the version.
Nothing is installed globally.
The parent terminal frontend still uses the standard library only.

The gem builds a native extension against SDL2, so its development headers have to be present:

| Platform | Install |
| --- | --- |
| Debian/Ubuntu | `sudo apt install libsdl2-dev` |
| Fedora | `sudo dnf install SDL2-devel` |
| macOS | `brew install sdl2` |

On macOS, Homebrew's `sdl2` currently resolves to sdl2-compat.
Its `sdl2-config` does not support the `--static-libs` flag that the build of `gosu` uses.
The extension then builds without its SDL2 and AppKit link line.
It fails at require time with a missing-symbol error.
Until Gosu handles sdl2-compat, use one of two ways around it:

- Use a real SDL2 from MacPorts or from a Homebrew bottle before sdl2-compat.
  The Homebrew command is `brew install sdl2 --formula`.
- Wrap `sdl2-config` so `--static-libs` answers with the `--libs` output.

## Why a window is affordable here

The terminal frontend exists because the Ruby backend only manages ~15 ticks/sec under YJIT.
That is far below what a GUI usually needs.
A terminal has orders of magnitude fewer cells to redraw than a window has pixels.
Here a window costs no more than a terminal: the per-frame work does not scale with the window.
The frame is sent to the GPU once as a 320x200 texture, and the GPU does the scaling.
So `--scale 6` costs exactly what `--scale 1` costs.

The module hands over a 640x400 framebuffer, but DOOM renders at 320x200 and scales up by exactly 2.
Its own start-up log reads `I_InitGraphics: Auto-scaling factor: 2`.
Halving it back therefore loses nothing.
It leaves 64000 pixels per frame to convert instead of 256000.
`FrameConverter` verifies that on the first frame rather than trusting it.
It falls back to full resolution if a future build of the module ever stops holding it.

What the window buys over the terminal is real key releases.
Terminals report presses only.
So the terminal frontend must generate a release after a hold window.
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
| `--fullscreen` | Start in full-screen mode. |
| `--smooth` | Scale the frame with interpolation instead of by nearest neighbor, saving ~10ms per frame. |
| `--smoke` | Headless self-check, described below. |

`./run.sh --smoke` needs no display.
It initializes the game and ticks it 60 times with no window.
It reports the tick rate and the per-frame conversion cost.
It sanity-checks the last frame, writes it to `screenshot.png`, and exits non-zero on failure.
This frontend writes a real PNG, since Gosu encodes PNGs without a drawing context.
The terminal frontend writes binary PPM instead, since Ruby's standard library has no PNG writer.

## Rendering

Handing a frame to Gosu takes two transformations per pixel.
Written as a straightforward Ruby loop, they would cost more than the wasm tick itself.
No other part of this frontend has that problem.
The module stores pixels as B,G,R,A while Gosu wants R,G,B,A.
The module's A byte is always 0, where Gosu needs 0xff.

Two facts about DOOM's renderer keep that to a few hundred Ruby-level operations per frame.
The count would otherwise be 64000.
It is paletted (VGA Mode 13h), with at most 256 colors per palette.
A session uses a few palettes.
So each distinct 32-bit source word is reordered once and cached.
A real frame holds around 175 of them.
Its 640x400 output is the 320x200 frame scaled up by exactly 2.
So each row is read with a single `unpack("Vx4" * 320)`.
There `V` takes a 32-bit pixel, and `x4` skips the repeated neighbor.
That leaves one `unpack`, one cached `map!` and one `pack` per row.
All of it runs at C level except the hash lookups.

Sending the result to the GPU is one `Gosu::Image.from_blob` per frame.
Gosu interpolates an image scaled past its native size.
The exception is an image whose texture was created with `retro: true`.
`Image.from_blob` has no parameter for that.
So the default path copies the frame into a nearest-neighbor render target, kept across frames.
That is what `--smooth` turns off, trading DOOM's sharp pixels for the ~10ms.

Measured on an x86-64 Linux container under Ruby 3.3.6 **without** YJIT, at the default `--scale 3`.
That build of Ruby has no YJIT support.

| Step | Cost per frame |
| --- | --- |
| Framebuffer conversion (640x400 BGRA to 320x200 RGBA) | 9.9ms |
| `Gosu::Image.from_blob` | 2.0ms |
| Nearest-neighbor copy (skipped by `--smooth`) | 10.0ms |
| One `tickGame` | 226ms |

The wasm tick dominates by an order of magnitude, which is the point.
Rendering is not what makes this frontend slow.
YJIT is what moves the tick figure.
`run.sh` always passes it, and `main.rb` warns on `stderr` if it ends up missing.
The terminal frontend measures the same Ruby backend at 15.9 ticks/sec with YJIT.
That figure is from an Apple Silicon Mac.

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
| F10 | Exit |

Gosu closes a window on Escape unless the frontend overrides its `button_down`.
This frontend does override it.
Escape belongs to DOOM's menu, and F10 or the window's close button exits the frontend.

Save games are written to `.savegame/` (ignored by Git) relative to wherever the script runs.

No sound: the module exposes no audio interface.
