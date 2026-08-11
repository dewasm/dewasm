// Command nes is an interactive frontend for the dewasm-generated NES library (nes_gen.go, produced from the agnes-based cache/nes.wasm by build.sh).
// Unlike the DOOM frontend, nes.wasm has zero host imports
// (there's nothing to wire up), so main.go only has to load a ROM into the module's linear memory and drive the game loop with ebiten for rendering and keyboard input.
//
// Run with -smoke for a headless self-check (no window): it inits the game, ticks it a few hundred times, and writes the last frame to screenshot.png.
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
	"github.com/hajimehoshi/ebiten/v2/vector"
)

// nesInst is package-level so runSmoke and the Game methods below can reach its exported funcs and memory without threading it through every call.
var nesInst *Nes

// frameW/frameH are fixed by the module (256x240) but read from its exports rather than hardcoded, matching frameBuf's DOOM-frontend counterpart.
// palette holds the module's fixed 64 colors as ready-to-copy opaque RGBA quads, decoded once from paletteOffset: the guest hands over one palette
// *index* per pixel, not a rendered image, so composing frameBuf is this frontend's job.
var (
	frameW, frameH int
	frameBuf       []byte
	palette        [64][4]byte
)

// defaultRomPath resolves the demo ROM relative to the built binary's own location rather than the current working directory: build.sh always produces ./nes inside examples/nes/go, so walking two directories up from there reaches examples/apps/cache regardless of where the binary is invoked from (mirroring how build.sh itself locates the repo root via
// `cd "$(dirname "$0")"`).
func defaultRomPath() string {
	exe, err := os.Executable()
	if err != nil {
		return "../apps/cache/alter_ego.nes"
	}
	exe, err = filepath.EvalSymlinks(exe)
	if err != nil {
		return "../apps/cache/alter_ego.nes"
	}
	return filepath.Join(filepath.Dir(exe), "..", "..", "apps", "cache", "alter_ego.nes")
}

// Button bits accepted by the module's setInput export.
const (
	btnA      = 1 << 0
	btnB      = 1 << 1
	btnSelect = 1 << 2
	btnStart  = 1 << 3
	btnUp     = 1 << 4
	btnDown   = 1 << 5
	btnLeft   = 1 << 6
	btnRight  = 1 << 7
)

// buttonMask reads currently-held keys into the bitmask setInput expects.
// Unlike DOOM's discrete reportKeyDown/reportKeyUp events, the NES module wants the full held-button state on every tick.
func buttonMask() uint32 {
	var mask uint32
	if ebiten.IsKeyPressed(ebiten.KeyArrowUp) {
		mask |= btnUp
	}
	if ebiten.IsKeyPressed(ebiten.KeyArrowDown) {
		mask |= btnDown
	}
	if ebiten.IsKeyPressed(ebiten.KeyArrowLeft) {
		mask |= btnLeft
	}
	if ebiten.IsKeyPressed(ebiten.KeyArrowRight) {
		mask |= btnRight
	}
	if ebiten.IsKeyPressed(ebiten.KeyX) {
		mask |= btnA
	}
	if ebiten.IsKeyPressed(ebiten.KeyZ) {
		mask |= btnB
	}
	if ebiten.IsKeyPressed(ebiten.KeyEnter) {
		mask |= btnStart
	}
	if ebiten.IsKeyPressed(ebiten.KeySpace) {
		mask |= btnSelect
	}
	return mask
}

// drawFrame re-fetches nesInst.memory.data on every call rather than caching it: a preceding tick can grow the module's memory, which replaces the backing slice (same caveat as DOOM's hostDrawFrame).
func drawFrame(setInput func(uint32), tickGame func(), screenOffset func() uint32) {
	setInput(buttonMask())
	tickGame()
	data := nesInst.memory.data
	off := int(screenOffset())
	n := frameW * frameH
	for i := 0; i < n; i++ {
		// One byte per pixel, a palette index; the & 0x3f mask is load-bearing (see examples/apps/src/nes_demo.c).
		copy(frameBuf[i*4:], palette[data[off+i]&0x3f][:])
	}
}

// controlsText mirrors the key mapping in buttonMask; shown as an on-screen overlay since there's no other discoverability path for a window app.
const controlsText = "arrows d-pad  x A  z B  enter start  space select  esc quit"

// titleUpdateEvery throttles ebiten.SetWindowTitle calls: the title only needs to be legible, not frame-accurate, and OS window-title updates are not free every tick.
const titleUpdateEvery = 60 // roughly once a second at the NES's ~60Hz frame rate

// Game implements ebiten.Game.
// The three exported funcs are type-asserted once in main rather than on every call.
type Game struct {
	setInput     func(uint32)
	tickGame     func()
	screenOffset func() uint32

	ticks int
}

func (g *Game) Update() error {
	if ebiten.IsKeyPressed(ebiten.KeyEscape) {
		return ebiten.Termination
	}
	drawFrame(g.setInput, g.tickGame, g.screenOffset)

	g.ticks++
	if g.ticks%titleUpdateEvery == 0 {
		ebiten.SetWindowTitle(fmt.Sprintf("NES (dewasm) - %.1f FPS", ebiten.ActualFPS()))
	}
	return nil
}

func (g *Game) Draw(screen *ebiten.Image) {
	if frameBuf != nil {
		screen.WritePixels(frameBuf)
	}
	drawHUD(screen, ebiten.ActualFPS())
}

// drawHUD overlays the FPS and control scheme on a dark bar along the bottom edge of the frame, so both stay legible against the game's own
// (highly variable) palette.
func drawHUD(screen *ebiten.Image, fps float64) {
	bounds := screen.Bounds()
	const barHeight = 16
	barY := float32(bounds.Dy() - barHeight)
	vector.FillRect(screen, 0, barY, float32(bounds.Dx()), barHeight, color.NRGBA{0, 0, 0, 180}, false)
	ebitenutil.DebugPrintAt(screen, fmt.Sprintf("%.0f FPS  |  %s", fps, controlsText), 4, bounds.Dy()-barHeight+2)
}

// Layout reports the module's native resolution as ebiten's logical screen size; ebiten upscales that to whatever the actual window size is, so Draw can hand it the framebuffer unscaled.
func (g *Game) Layout(outsideWidth, outsideHeight int) (int, int) {
	return frameW, frameH
}

// loadRom reads the ROM file, copies it into the module's linear memory via allocRom, and returns the game-initialized Game ready for the loop.
func loadRom(romPath string) *Game {
	rom, err := os.ReadFile(romPath)
	if err != nil {
		fmt.Fprintln(os.Stderr, "nes: reading ROM:", err)
		os.Exit(1)
	}

	nesInst = NewNes(nil, nil, nil, nil)
	allocRom := nesInst.Exports["allocRom"].(func(uint32) uint32)
	initGame := nesInst.Exports["initGame"].(func() uint32)
	setInput := nesInst.Exports["setInput"].(func(uint32))
	tickGame := nesInst.Exports["tickGame"].(func())
	screenOffset := nesInst.Exports["screenOffset"].(func() uint32)
	paletteOffset := nesInst.Exports["paletteOffset"].(func() uint32)
	frameWidth := nesInst.Exports["frameWidth"].(func() uint32)
	frameHeight := nesInst.Exports["frameHeight"].(func() uint32)

	ptr := allocRom(uint32(len(rom)))
	copy(nesInst.memory.data[ptr:], rom)

	if ok := initGame(); ok != 1 {
		fmt.Fprintln(os.Stderr, "nes: initGame failed (ROM rejected or unsupported mapper)")
		os.Exit(1)
	}

	frameW, frameH = int(frameWidth()), int(frameHeight())
	frameBuf = make([]byte, frameW*frameH*4)

	// The palette is fixed data (R,G,B,A, alpha padding), so decode it once into opaque RGBA quads; only the index buffer changes per frame.
	poff := int(paletteOffset())
	for i := range palette {
		c := nesInst.memory.data[poff+i*4:]
		palette[i] = [4]byte{c[0], c[1], c[2], 0xff}
	}

	return &Game{setInput: setInput, tickGame: tickGame, screenOffset: screenOffset}
}

// runSmoke drives the game headlessly: no window, no ebiten loop.
// It exists so CI and quick local checks can confirm the generated library still links and produces a plausible frame without a display.
func runSmoke(g *Game) {
	const ticks = 300
	start := time.Now()
	for i := 0; i < ticks; i++ {
		drawFrame(g.setInput, g.tickGame, g.screenOffset)
	}
	elapsed := time.Since(start)
	rate := float64(ticks) / elapsed.Seconds()
	fmt.Printf("smoke: ran %d ticks in %s (%.1f ticks/sec)\n", ticks, elapsed, rate)

	distinct := make(map[uint32]struct{})
	for i := 0; i+4 <= len(frameBuf); i += 4 {
		distinct[binary.LittleEndian.Uint32(frameBuf[i:i+4])] = struct{}{}
	}
	fmt.Printf("smoke: final frame is %dx%d with %d distinct colors\n", frameW, frameH, len(distinct))
	// The NES's PPU palette tops out at 64 colors total, so a healthy frame lands in the dozens; a degenerate (blank/solid) frame lands at 1.
	const minDistinctColors = 4
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

	romPath := defaultRomPath()
	if flag.NArg() > 0 {
		romPath = flag.Arg(0)
	}

	game := loadRom(romPath)

	if *smoke {
		runSmoke(game)
		return
	}

	ebiten.SetWindowSize(frameW*2, frameH*2)
	ebiten.SetWindowTitle("NES (dewasm)")
	// The NTSC NES runs at ~60.0988 Hz; ebiten's default 60 TPS is close enough that no explicit SetTPS call is needed (unlike DOOM's 35Hz, which does need an explicit override).
	if err := ebiten.RunGame(game); err != nil {
		fmt.Fprintln(os.Stderr, "nes:", err)
		os.Exit(1)
	}
}
