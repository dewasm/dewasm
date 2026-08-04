// Command doom is an interactive frontend for the dewasm-generated DOOM
// library (doom_gen.go, produced from jacobenget/doom.wasm by build.sh). It
// wires the wasm module's ten host imports to real OS facilities (stdio,
// save-game files, a monotonic clock) and drives the game loop with ebiten
// for rendering and keyboard input.
//
// Run with -smoke for a headless self-check (no window): it inits the game,
// ticks it a few hundred times, and writes the last frame to screenshot.png.
package main

import (
	"encoding/binary"
	"flag"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"time"

	"github.com/hajimehoshi/ebiten/v2"
	"github.com/hajimehoshi/ebiten/v2/ebitenutil"
	"github.com/hajimehoshi/ebiten/v2/inpututil"
	"github.com/hajimehoshi/ebiten/v2/vector"
)

// doomInst is package-level because the host import closures below need to
// read/write its memory, but Go requires the closures to exist before
// NewDoom returns the instance that owns that memory.
var doomInst *Doom

// frameW/frameH/frameBuf hold the most recent frame ui.drawFrame delivered,
// already swizzled from the module's B,G,R,A byte order into ebiten's
// R,G,B,A. They're sized once loading.onGameInit reports the real
// dimensions (640x400 for this binary), never hardcoded.
var (
	frameW, frameH int
	frameBuf       []byte
)

var startTime = time.Now()

const saveDir = ".savegame"

func saveGamePath(id uint32) string {
	return filepath.Join(saveDir, fmt.Sprintf("doomsav%d.dsg", id))
}

// readString decodes a UTF-8 string out of the module's linear memory. It's
// a stand-in for the `Memory.read_string` helper dewasm only emits for
// WASI-shaped modules; this one has no WASI imports.
func readString(off, length uint32) string {
	return string(doomInst.memory.data[off : off+length])
}

// Doom's messages carry no trailing newline; the reference embedder prints one per message.
func hostOnErrorMessage(off, length uint32) {
	fmt.Fprintln(os.Stderr, readString(off, length))
}

func hostOnInfoMessage(off, length uint32) {
	fmt.Fprintln(os.Stdout, readString(off, length))
}

func hostSizeOfSaveGame(id uint32) uint32 {
	info, err := os.Stat(saveGamePath(id))
	if err != nil {
		return 0
	}
	return uint32(info.Size())
}

func hostReadSaveGame(id, dstOff uint32) uint32 {
	b, err := os.ReadFile(saveGamePath(id))
	if err != nil {
		return 0
	}
	copy(doomInst.memory.data[dstOff:], b)
	return uint32(len(b))
}

func hostWriteSaveGame(id, srcOff, length uint32) uint32 {
	if err := os.MkdirAll(saveDir, 0o755); err != nil {
		return 0
	}
	b := doomInst.memory.data[srcOff : srcOff+length]
	if err := os.WriteFile(saveGamePath(id), b, 0o644); err != nil {
		return 0
	}
	return length
}

// hostTimeInMilliseconds backs DOOM's internal 35Hz pacing, so it must be a
// real monotonic clock (not a fake stepped one) or the game's notion of
// elapsed time would drift from ebiten's own frame timing.
func hostTimeInMilliseconds() uint64 {
	return uint64(time.Since(startTime).Milliseconds())
}

// hostDrawFrame re-fetches doomInst.memory.data on every call rather than
// caching it: a preceding tick can grow the module's memory, which replaces
// the backing slice.
func hostDrawFrame(bufOff uint32) {
	data := doomInst.memory.data
	n := frameW * frameH
	for i := 0; i < n; i++ {
		o := int(bufOff) + i*4
		b, g, r := data[o], data[o+1], data[o+2]
		j := i * 4
		frameBuf[j+0] = r
		frameBuf[j+1] = g
		frameBuf[j+2] = b
		frameBuf[j+3] = 0xff // force opaque; the source alpha byte is not meaningful here
	}
}

// hostReadWads is never invoked: hostWadSizes leaves both output slots at
// their pre-zeroed 0, which selects the wasm-embedded shareware WAD and
// skips the readWads call entirely.
func hostReadWads(dstOff, lengthsArrOff uint32) {}

// hostWadSizes intentionally does nothing. Leaving the wad-count and
// total-byte-size outputs untouched (they arrive pre-zeroed) tells the
// module to fall back to its embedded shareware WAD; a --wads flag to
// supply external WADs is out of scope.
func hostWadSizes(numberOfWadsOff, totalBytesOff uint32) {}

func hostOnGameInit(width, height uint32) {
	frameW, frameH = int(width), int(height)
	frameBuf = make([]byte, frameW*frameH*4)
}

func buildImports() Imports {
	return Imports{
		"console": map[string]any{
			"onErrorMessage": hostOnErrorMessage,
			"onInfoMessage":  hostOnInfoMessage,
		},
		"gameSaving": map[string]any{
			"sizeOfSaveGame": hostSizeOfSaveGame,
			"readSaveGame":   hostReadSaveGame,
			"writeSaveGame":  hostWriteSaveGame,
		},
		"runtimeControl": map[string]any{
			"timeInMilliseconds": hostTimeInMilliseconds,
		},
		"ui": map[string]any{
			"drawFrame": hostDrawFrame,
		},
		"loading": map[string]any{
			"onGameInit": hostOnGameInit,
			"wadSizes":   hostWadSizes,
			"readWads":   hostReadWads,
		},
	}
}

// keyCode returns the numeric key code behind one of the module's exported
// KEY_* globals (e.g. "KEY_LEFTARROW"). Those exports are *global[uint32]
// values; .value is unexported but reachable because main.go and doom_gen.go
// share a package.
func keyCode(name string) uint32 {
	return doomInst.Exports[name].(*global[uint32]).value
}

// mapKey translates an ebiten key to the code reportKeyDown/reportKeyUp
// expect: the module's KEY_* constant for special keys, or the ASCII value
// of the lowercase, unmodified character for letters and digits (used for
// text entry and, notably, weapon-select digits).
func mapKey(k ebiten.Key) (uint32, bool) {
	switch k {
	case ebiten.KeyArrowLeft:
		return keyCode("KEY_LEFTARROW"), true
	case ebiten.KeyArrowRight:
		return keyCode("KEY_RIGHTARROW"), true
	case ebiten.KeyArrowUp:
		return keyCode("KEY_UPARROW"), true
	case ebiten.KeyArrowDown:
		return keyCode("KEY_DOWNARROW"), true
	case ebiten.KeyControlLeft, ebiten.KeyControlRight:
		return keyCode("KEY_FIRE"), true
	case ebiten.KeySpace:
		return keyCode("KEY_USE"), true
	case ebiten.KeyShiftLeft, ebiten.KeyShiftRight:
		return keyCode("KEY_SHIFT"), true
	case ebiten.KeyTab:
		return keyCode("KEY_TAB"), true
	case ebiten.KeyEscape:
		return keyCode("KEY_ESCAPE"), true
	case ebiten.KeyEnter:
		return keyCode("KEY_ENTER"), true
	case ebiten.KeyBackspace:
		return keyCode("KEY_BACKSPACE"), true
	case ebiten.KeyAltLeft, ebiten.KeyAltRight:
		return keyCode("KEY_ALT"), true
	case ebiten.KeyComma:
		return keyCode("KEY_STRAFE_L"), true
	case ebiten.KeyPeriod:
		return keyCode("KEY_STRAFE_R"), true
	}
	if k >= ebiten.KeyA && k <= ebiten.KeyZ {
		return uint32('a') + uint32(k-ebiten.KeyA), true
	}
	if k >= ebiten.Key0 && k <= ebiten.Key9 {
		return uint32('0') + uint32(k-ebiten.Key0), true
	}
	return 0, false
}

// controlsText mirrors the key mapping in mapKey; shown as an on-screen
// overlay since there's no other discoverability path for a window app.
const controlsText = "arrows move  ctrl fire  space use  shift run  tab automap  ,/. strafe  1-7 weapon  esc menu"

// titleUpdateEvery throttles ebiten.SetWindowTitle calls: the title only
// needs to be legible, not frame-accurate, and OS window-title updates are
// not free every tick.
const titleUpdateEvery = 35 // roughly once a second at DOOM's 35 TPS

// Game implements ebiten.Game. tickGame/reportKeyDown/reportKeyUp are the
// module's exported funcs, type-asserted once in main rather than on every
// call.
type Game struct {
	tickGame      func()
	reportKeyDown func(uint32)
	reportKeyUp   func(uint32)

	ticks int
}

func (g *Game) Update() error {
	for _, k := range inpututil.AppendJustPressedKeys(nil) {
		if code, ok := mapKey(k); ok {
			g.reportKeyDown(code)
		}
	}
	for _, k := range inpututil.AppendJustReleasedKeys(nil) {
		if code, ok := mapKey(k); ok {
			g.reportKeyUp(code)
		}
	}
	g.tickGame() // drives ui.drawFrame internally, refreshing frameBuf

	g.ticks++
	if g.ticks%titleUpdateEvery == 0 {
		ebiten.SetWindowTitle(fmt.Sprintf("DOOM (dewasm) - %.1f FPS", ebiten.ActualFPS()))
	}
	return nil
}

func (g *Game) Draw(screen *ebiten.Image) {
	if frameBuf != nil {
		screen.WritePixels(frameBuf)
	}
	drawHUD(screen, ebiten.ActualFPS())
}

// drawHUD overlays the FPS and control scheme on a dark bar along the
// bottom edge of the frame, so both stay legible against DOOM's own
// (highly variable) palette.
func drawHUD(screen *ebiten.Image, fps float64) {
	bounds := screen.Bounds()
	const barHeight = 16
	barY := float32(bounds.Dy() - barHeight)
	vector.FillRect(screen, 0, barY, float32(bounds.Dx()), barHeight, color.NRGBA{0, 0, 0, 180}, false)
	ebitenutil.DebugPrintAt(screen, fmt.Sprintf("%.0f FPS  |  %s", fps, controlsText), 4, bounds.Dy()-barHeight+2)
}

// Layout reports the module's native resolution as ebiten's logical screen
// size; ebiten upscales that to whatever the actual window size is, so Draw
// can hand it the framebuffer unscaled.
func (g *Game) Layout(outsideWidth, outsideHeight int) (int, int) {
	return frameW, frameH
}

// runSmoke drives the game headlessly: no window, no ebiten loop. It exists
// so CI and quick local checks can confirm the generated library still
// links and produces a plausible frame without a display.
func runSmoke(tickGame func()) {
	const ticks = 300
	start := time.Now()
	for i := 0; i < ticks; i++ {
		tickGame()
	}
	elapsed := time.Since(start)
	rate := float64(ticks) / elapsed.Seconds()
	fmt.Printf("smoke: ran %d ticks in %s (%.1f ticks/sec)\n", ticks, elapsed, rate)

	if frameBuf == nil {
		fmt.Fprintln(os.Stderr, "smoke: FAIL: no frame was ever captured")
		os.Exit(1)
	}

	distinct := make(map[uint32]struct{})
	for i := 0; i+4 <= len(frameBuf); i += 4 {
		distinct[binary.LittleEndian.Uint32(frameBuf[i:i+4])] = struct{}{}
	}
	fmt.Printf("smoke: final frame is %dx%d with %d distinct colors\n", frameW, frameH, len(distinct))
	// DOOM's software renderer is paletted (classic VGA Mode 13h: at most
	// 256 colors), so a healthy frame tops out in the low hundreds, not
	// the thousands a truecolor renderer would produce. A degenerate
	// frame (blank/solid) instead lands in the single digits.
	const minDistinctColors = 50
	if len(distinct) <= minDistinctColors {
		fmt.Fprintln(os.Stderr, "smoke: FAIL: frame looks degenerate (too few distinct colors)")
		os.Exit(1)
	}

	if err := writePNG("screenshot.png", frameW, frameH, frameBuf); err != nil {
		fmt.Fprintln(os.Stderr, "smoke: FAIL: writing screenshot.png:", err)
		os.Exit(1)
	}
	fmt.Println("smoke: wrote screenshot.png")
}

func writePNG(path string, w, h int, rgba []byte) error {
	img := &image.RGBA{Pix: rgba, Stride: w * 4, Rect: image.Rect(0, 0, w, h)}
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	defer f.Close()
	return png.Encode(f, img)
}

func main() {
	smoke := flag.Bool("smoke", false, "headless self-check: init, tick ~300 times, write screenshot.png, exit")
	flag.Parse()

	doomInst = NewDoom(buildImports(), nil, nil, nil)
	initGame := doomInst.Exports["initGame"].(func())
	tickGame := doomInst.Exports["tickGame"].(func())
	initGame() // triggers loading.onGameInit, which sizes frameW/frameH/frameBuf

	if *smoke {
		runSmoke(tickGame)
		return
	}

	reportKeyDown := doomInst.Exports["reportKeyDown"].(func(uint32))
	reportKeyUp := doomInst.Exports["reportKeyUp"].(func(uint32))

	ebiten.SetWindowSize(frameW*2, frameH*2)
	ebiten.SetWindowTitle("DOOM (dewasm)")
	// 35 TPS matches DOOM's own internal tic rate exactly, so every
	// Update() call advances exactly one game tic instead of some being
	// no-ops (tickGame paces itself off runtimeControl.timeInMilliseconds
	// regardless of how often it's called).
	ebiten.SetTPS(35)

	game := &Game{tickGame: tickGame, reportKeyDown: reportKeyDown, reportKeyUp: reportKeyUp}
	if err := ebiten.RunGame(game); err != nil {
		fmt.Fprintln(os.Stderr, "doom:", err)
		os.Exit(1)
	}
}
