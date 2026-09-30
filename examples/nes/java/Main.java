// NES frontend for the dewasm-generated Nes.java library.
// It is in the default package, same as Nes.java.
// So Nes and its nested Nes.Rt/Nes.Memory classes are reachable without an import.
// Two entry points share one engine: an interactive Swing window (default) and a smoke test.
// The smoke test (`--smoke`) is headless.
// It never touches java.awt.event/javax.swing, so it can run without a display.
//
// `nes.wasm` has zero imports, unlike DOOM's `console`/`gameSaving`/`ui`/`loading` host interface.
// It exports memory plus `allocRom`/`initGame`/`setInput`/`tickGame` and the frame accessors.
// It never calls back into the host.
// So unlike DoomEngine, NesEngine builds no imports map at all.
// It just drives the exports and composes the frame out of linear memory itself after every tick.
// The host is responsible for the fixed 60 Hz pace.
// `tickGame` renders exactly one NES video frame per call, with no internal timing of its own.

import java.awt.Color;
import java.awt.Dimension;
import java.awt.Graphics;
import java.awt.Graphics2D;
import java.awt.RenderingHints;
import java.awt.event.KeyEvent;
import java.awt.event.KeyListener;
import java.awt.event.WindowAdapter;
import java.awt.event.WindowEvent;
import java.awt.image.BufferedImage;
import java.awt.image.DataBufferInt;
import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashSet;
import java.util.Set;
import java.util.concurrent.atomic.AtomicInteger;
import javax.imageio.ImageIO;
import javax.swing.JFrame;
import javax.swing.JPanel;
import javax.swing.SwingUtilities;

public class Main {

    // Alter Ego by Shiru, released into the public domain.
    // `examples/apps/scripts/nes.sh` fetches it and checks it against a fixed checksum.
    // It is resolved relative to the working directory.
    // run.sh/build.sh leave that at this script's own directory.
    private static final String DEFAULT_ROM = "../../apps/cache/alter_ego.nes";

    // Button bit mask (matches `setInput` in `nes_demo.c` and the README table).
    private static final int BTN_A = 1;
    private static final int BTN_B = 2;
    private static final int BTN_SELECT = 4;
    private static final int BTN_START = 8;
    private static final int BTN_UP = 16;
    private static final int BTN_DOWN = 32;
    private static final int BTN_LEFT = 64;
    private static final int BTN_RIGHT = 128;

    private static final int FPS = 60;

    // Shown on the screen over the game (mirrors `mapKey`).
    // There's no other discoverability path for a window app.
    private static final String CONTROLS_TEXT = "arrows d-pad  x A  z B  enter start  space select  esc quit";

    private static final String WINDOW_TITLE = "NES (dewasm) - Alter Ego";

    public static void main(String[] args) throws Exception {
        boolean smoke = false;
        String romPath = DEFAULT_ROM;
        for (String arg : args) {
            if (arg.equals("--smoke")) {
                smoke = true;
            } else {
                romPath = arg;
            }
        }
        byte[] rom = Files.readAllBytes(Path.of(romPath));
        if (smoke) {
            runSmoke(rom);
        } else {
            runGui(rom);
        }
    }

    // Headless self-test: initialization + a fixed number of ticks with no window.
    // It uses no KeyListener and no JFrame.
    // It uses only BufferedImage/ImageIO, which render in software and need no display.
    // Alter Ego opens on a run of static black-background credits screens.
    // A few Start presses are injected along the way to page through them.
    // They reach the animated title screen.
    // This mirrors DOOM's smoke test, which injects Enter to clear its title/legal screens.
    // So the final frame has real varied pixel art rather than mostly-black credits text.
    // That's what the distinct-color sanity check below is actually probing for.
    private static void runSmoke(byte[] rom) throws IOException {
        NesEngine engine = new NesEngine(rom);

        int ticks = 600;
        long t0 = System.nanoTime();
        for (int i = 0; i < ticks; i++) {
            int phase = i % 40;
            int buttons = phase < 4 ? BTN_START : 0;
            engine.tick(buttons);
        }
        double elapsedSec = (System.nanoTime() - t0) / 1e9;
        double ticksPerSec = ticks / elapsedSec;
        System.out.printf("smoke: %d ticks in %.3fs (%.1f ticks/sec)%n", ticks, elapsedSec, ticksPerSec);

        BufferedImage frame = engine.frame;
        File screenshot = new File("screenshot.png");
        ImageIO.write(frame, "png", screenshot);
        System.out.println("smoke: wrote " + screenshot.getAbsolutePath());

        Set<Integer> colors = new HashSet<>();
        for (int y = 0; y < frame.getHeight(); y++) {
            for (int x = 0; x < frame.getWidth(); x++) {
                colors.add(frame.getRGB(x, y));
            }
        }
        System.out.println("smoke: " + colors.size() + " distinct colors in final frame");
        // A blank/solid-color buffer would top out at a few colors.
        // Such a buffer would mean the memory read was set up wrong.
        // Any real rendered NES frame clears this easily.
        // Such a frame is a 25-entry palette shaded across a title screen or a game scene.
        if (colors.size() <= 8) {
            System.err.println("smoke: FAILED sanity check (expected > 8 distinct colors)");
            System.exit(1);
        }
        System.out.println("smoke: OK");
    }

    // Interactive window: a `JFrame` whose panel draws the engine's current `BufferedImage`.
    // The image is scaled ~2x, and a `KeyListener` tracks which mapped keys are held.
    // NES ticks on its own thread, paced to 60 Hz, while Swing delivers input on the EDT.
    // `tickGame` has no internal timing.
    // One call renders exactly one video frame, however fast it's called.
    // The game thread reads the held-key set each tick.
    // A simple lock guards the set, since it's small and touched every ~16ms either way.
    private static void runGui(byte[] rom) throws IOException {
        JFrame window = new JFrame(WINDOW_TITLE);
        NesPanel panel = new NesPanel();
        window.setContentPane(panel);
        window.setDefaultCloseOperation(JFrame.EXIT_ON_CLOSE);

        NesEngine engine = new NesEngine(rom);
        panel.engine = engine;
        panel.controlsText = CONTROLS_TEXT;
        panel.setPreferredSize(new Dimension(engine.width * 2, engine.height * 2));

        Object heldLock = new Object();
        Set<Integer> held = new HashSet<>();

        window.addKeyListener(new KeyListener() {
            @Override
            public void keyTyped(KeyEvent e) {
            }

            @Override
            public void keyPressed(KeyEvent e) {
                if (e.getKeyCode() == KeyEvent.VK_ESCAPE) {
                    window.dispatchEvent(new WindowEvent(window, WindowEvent.WINDOW_CLOSING));
                    return;
                }
                Integer bit = mapKey(e);
                if (bit != null) {
                    synchronized (heldLock) {
                        held.add(bit);
                    }
                }
            }

            @Override
            public void keyReleased(KeyEvent e) {
                Integer bit = mapKey(e);
                if (bit != null) {
                    synchronized (heldLock) {
                        held.remove(bit);
                    }
                }
            }
        });
        window.setFocusTraversalKeysEnabled(false);
        window.setFocusable(true);

        AtomicInteger running = new AtomicInteger(1);
        Thread gameThread = new Thread(() -> {
            long frameNanos = 1_000_000_000L / FPS;
            long next = System.nanoTime();
            // Measured over ~1s windows (frame counting), not one frame at a time.
            // A rate measured per frame would be far too noisy to read.
            // That holds even though `tickGame` paces at a fixed 60Hz target.
            long fpsWindowStart = next;
            int fpsWindowFrames = 0;
            while (running.get() != 0) {
                int buttons;
                synchronized (heldLock) {
                    buttons = 0;
                    for (int bit : held) {
                        buttons |= bit;
                    }
                }
                engine.tick(buttons);
                panel.repaint();

                fpsWindowFrames++;
                long now = System.nanoTime();
                double elapsedSec = (now - fpsWindowStart) / 1e9;
                if (elapsedSec >= 1.0) {
                    double measuredFps = fpsWindowFrames / elapsedSec;
                    panel.fps = measuredFps;
                    SwingUtilities.invokeLater(
                        () -> window.setTitle(String.format("%s - %.1f FPS", WINDOW_TITLE, measuredFps)));
                    fpsWindowStart = now;
                    fpsWindowFrames = 0;
                }

                next += frameNanos;
                long sleepNanos = next - System.nanoTime();
                if (sleepNanos > 0) {
                    try {
                        Thread.sleep(sleepNanos / 1_000_000L, (int) (sleepNanos % 1_000_000L));
                    } catch (InterruptedException e) {
                        Thread.currentThread().interrupt();
                        break;
                    }
                } else {
                    // Fell behind (for example the window was minimized).
                    // Resync instead of a catch-up burst that would only fall further behind.
                    next = System.nanoTime();
                }
            }
        }, "nes-game-thread");
        gameThread.setDaemon(true);

        window.addWindowListener(new WindowAdapter() {
            @Override
            public void windowClosing(WindowEvent e) {
                running.set(0);
                gameThread.interrupt();
            }
        });

        window.pack();
        window.setLocationRelativeTo(null);
        window.setVisible(true);
        window.requestFocusInWindow();
        gameThread.start();
    }

    // Arrows = D-pad, X = A, Z = B, Enter = Start, Space = Select.
    // Returns null for keys with no NES mapping.
    private static Integer mapKey(KeyEvent e) {
        switch (e.getKeyCode()) {
            case KeyEvent.VK_LEFT:
                return BTN_LEFT;
            case KeyEvent.VK_RIGHT:
                return BTN_RIGHT;
            case KeyEvent.VK_UP:
                return BTN_UP;
            case KeyEvent.VK_DOWN:
                return BTN_DOWN;
            case KeyEvent.VK_X:
                return BTN_A;
            case KeyEvent.VK_Z:
                return BTN_B;
            case KeyEvent.VK_ENTER:
                return BTN_START;
            case KeyEvent.VK_SPACE:
                return BTN_SELECT;
            default:
                return null;
        }
    }

    // Draws the engine's current frame scaled to the panel, preserving aspect ratio.
    // Black bars fill the short axis.
    private static final class NesPanel extends JPanel {
        volatile NesEngine engine;
        volatile double fps;
        volatile String controlsText = "";

        NesPanel() {
            setBackground(Color.BLACK);
            setFocusable(true);
        }

        @Override
        protected void paintComponent(Graphics g) {
            super.paintComponent(g);
            NesEngine e = engine;
            BufferedImage frame = e == null ? null : e.frame;
            if (frame == null) {
                return;
            }
            int pw = getWidth();
            int ph = getHeight();
            double scale = Math.min((double) pw / frame.getWidth(), (double) ph / frame.getHeight());
            int dw = (int) Math.round(frame.getWidth() * scale);
            int dh = (int) Math.round(frame.getHeight() * scale);
            int dx = (pw - dw) / 2;
            int dy = (ph - dh) / 2;
            Graphics2D g2 = (Graphics2D) g;
            g2.setRenderingHint(RenderingHints.KEY_INTERPOLATION, RenderingHints.VALUE_INTERPOLATION_NEAREST_NEIGHBOR);
            g2.drawImage(frame, dx, dy, dw, dh, null);
            drawHud(g2, dx, dy + dh);
        }

        // Draws the FPS and control scheme over the frame, on a fixed-color dark bar.
        // The bar runs along the bottom edge of the rendered frame.
        // So both stay readable against the game's own (highly variable) palette.
        // Without the bar, the panel's plain black background would bleed through.
        private void drawHud(Graphics2D g2, int frameLeft, int frameBottom) {
            String text = String.format("%.0f FPS  |  %s", fps, controlsText);
            int barHeight = 18;
            int barTop = frameBottom - barHeight;
            g2.setColor(new Color(0, 0, 0, 180));
            g2.fillRect(frameLeft, barTop, getWidth() - 2 * frameLeft, barHeight);
            g2.setColor(Color.WHITE);
            g2.drawString(text, frameLeft + 4, frameBottom - 5);
        }
    }

    // Owns the Nes instance and the current framebuffer.
    // nes.wasm has no imports, so construction just needs the ROM bytes.
    // It allocates a guest buffer with `allocRom`, copies the ROM in, and calls `initGame`.
    // Every tick() sets the input mask and advances one video frame.
    // Then it composes the guest's frame into the backing `int[]` of the `BufferedImage`.
    // The guest hands over one palette *index* per pixel plus a fixed 64-entry palette.
    // So the palette is decoded once into the ARGB `int` values `TYPE_INT_ARGB` expects.
    // Every pixel is then one masked table lookup.
    private static final class NesEngine {
        final Nes nes;
        final Nes.Memory memory;
        final Nes.Rt.Fn setInputFn;
        final Nes.Rt.Fn tickGameFn;
        final int screenOff;
        final int[] palette = new int[64];

        final int width;
        final int height;
        final BufferedImage frame;

        NesEngine(byte[] rom) {
            // This module has zero wasm imports (verified by `nes.sh` with `wasm-objdump`).
            // It has no WASI imports either.
            // So `null` is accepted for the imports map and for arguments/environment/preopens.
            this.nes = new Nes(null, null, null, null);
            this.memory = (Nes.Memory) nes.Exports.get("memory");

            Nes.Rt.Fn allocRomFn = (Nes.Rt.Fn) nes.Exports.get("allocRom");
            Nes.Rt.Fn initGameFn = (Nes.Rt.Fn) nes.Exports.get("initGame");
            this.setInputFn = (Nes.Rt.Fn) nes.Exports.get("setInput");
            this.tickGameFn = (Nes.Rt.Fn) nes.Exports.get("tickGame");
            Nes.Rt.Fn screenOffsetFn = (Nes.Rt.Fn) nes.Exports.get("screenOffset");
            Nes.Rt.Fn paletteOffsetFn = (Nes.Rt.Fn) nes.Exports.get("paletteOffset");
            Nes.Rt.Fn frameWidthFn = (Nes.Rt.Fn) nes.Exports.get("frameWidth");
            Nes.Rt.Fn frameHeightFn = (Nes.Rt.Fn) nes.Exports.get("frameHeight");

            int ptr = (Integer) allocRomFn.invoke(new Object[] { rom.length });
            memory.init(Integer.toUnsignedLong(ptr), rom, 0, rom.length);

            int ok = (Integer) initGameFn.invoke(new Object[0]);
            if (ok != 1) {
                throw new IllegalStateException("initGame failed (bad or unsupported ROM)");
            }

            this.width = (Integer) frameWidthFn.invoke(new Object[0]);
            this.height = (Integer) frameHeightFn.invoke(new Object[0]);
            this.frame = new BufferedImage(width, height, BufferedImage.TYPE_INT_ARGB);

            // Both offsets are stable for the emulator's lifetime.
            // The palette is fixed data (R,G,B,A, alpha padding), so it is decoded once.
            this.screenOff = (Integer) screenOffsetFn.invoke(new Object[0]);
            int poff = (Integer) paletteOffsetFn.invoke(new Object[0]);
            for (int i = 0; i < palette.length; i++) {
                palette[i] = 0xff000000
                    | ((memory.d[poff + i * 4] & 0xff) << 16)
                    | ((memory.d[poff + i * 4 + 1] & 0xff) << 8)
                    | (memory.d[poff + i * 4 + 2] & 0xff);
            }
        }

        void tick(int buttons) {
            setInputFn.invoke(new Object[] { buttons });
            tickGameFn.invoke(new Object[0]);

            int[] pixels = ((DataBufferInt) frame.getRaster().getDataBuffer()).getData();
            byte[] d = memory.d;
            for (int i = 0; i < pixels.length; i++) {
                // One byte per pixel, a palette index.
                // The `& 0x3f` mask is load-bearing (see `examples/apps/src/nes_demo.c`).
                pixels[i] = palette[d[screenOff + i] & 0x3f];
            }
        }
    }
}
