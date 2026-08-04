// NES frontend for the dewasm-generated Nes.java library (default package,
// same as Nes.java, so the Memory/Rt/Nes classes are reachable without an
// import). Two entry points share one engine: an interactive Swing window
// (default) and a headless smoke test (`--smoke`) that never touches
// java.awt.event/javax.swing so it can run without a display.
//
// nes.wasm has zero imports (unlike DOOM's console/gameSaving/ui/loading host
// interface): it exposes memory plus allocRom/initGame/setInput/tickGame and
// the frame accessors directly, and never calls back into the host. So unlike
// DoomEngine, NesEngine wires no imports map at all — it just drives the
// exports and composes the frame out of linear memory itself after every
// tick, at a fixed 60 Hz pace the host is responsible for (tickGame renders
// exactly one NES video frame per call, with no internal timing of its own).

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

    // Alter Ego by Shiru, released into the public domain (fetched/pinned by
    // examples/apps/scripts/nes.sh). Resolved relative to the working
    // directory, which run.sh/build.sh leave at this script's own directory.
    private static final String DEFAULT_ROM = "../../apps/cache/alter_ego.nes";

    // Button bitmask (matches nes_demo.c's setInput and the README table).
    private static final int BTN_A = 1;
    private static final int BTN_B = 2;
    private static final int BTN_SELECT = 4;
    private static final int BTN_START = 8;
    private static final int BTN_UP = 16;
    private static final int BTN_DOWN = 32;
    private static final int BTN_LEFT = 64;
    private static final int BTN_RIGHT = 128;

    private static final int FPS = 60;

    // Shown as an on-screen overlay (mirrors mapKey) since there's no other
    // discoverability path for a window app.
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

    // Headless self-test: init + a fixed number of ticks with no window, no
    // KeyListener, no JFrame — only BufferedImage/ImageIO, which render in
    // software and need no display. Alter Ego opens on a run of static
    // black-background credits screens; a handful of Start presses are
    // injected along the way (mirroring DOOM's smoke test injecting Enter to
    // clear its title/legal screens) to page through them into the animated
    // title screen, so the final frame has real varied pixel art rather than
    // mostly-black credits text — that's what the distinct-color sanity check
    // below is actually probing for.
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
        // A blank/solid-color buffer (the memory read wired up wrong) would
        // top out at a handful of colors; any real rendered NES frame (a
        // 25-entry palette shaded across a title screen or gameplay) clears
        // this easily.
        if (colors.size() <= 8) {
            System.err.println("smoke: FAILED sanity check (expected > 8 distinct colors)");
            System.exit(1);
        }
        System.out.println("smoke: OK");
    }

    // Interactive window: a JFrame whose panel blits the engine's current
    // BufferedImage scaled ~2x, plus a KeyListener that tracks which mapped
    // keys are held. NES ticks on its own thread, paced to 60 Hz (tickGame
    // has no internal timing — one call renders exactly one video frame,
    // however fast it's called), while Swing delivers input on the EDT; the
    // held-key set is read from the game thread each tick, guarded by a
    // simple lock since it's small and touched every ~16ms either way.
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
            // Measured over ~1s windows (frame counting), not one frame at a
            // time: a per-frame instantaneous rate would be far too noisy to
            // read even though tickGame paces at a fixed 60Hz target.
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
                    // Fell behind (e.g. the window was minimized): resync
                    // instead of a spiral-of-death catch-up burst.
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

    // Arrows = D-pad, X = A, Z = B, Enter = Start, Space = Select. Returns
    // null for keys with no NES mapping.
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

    // Draws the engine's current frame scaled to the panel, preserving
    // aspect ratio and letterboxing with black bars on the short axis.
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

        // Overlays the FPS and control scheme on a fixed-color dark bar
        // along the bottom edge of the rendered frame, so both stay legible
        // against the game's own (highly variable) palette rather than the
        // panel's plain black background bleeding through.
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

    // Owns the Nes instance and the current framebuffer. nes.wasm has no
    // imports, so construction just needs the ROM bytes: allocate a guest
    // buffer with allocRom, copy the ROM in, and initGame it. Every tick()
    // sets the input mask, advances one video frame, and composes the guest's
    // frame into the BufferedImage's backing int[] — the guest hands over one
    // palette *index* per pixel plus a fixed 64-entry palette (ADR-59), so the
    // palette is decoded once into the ARGB ints TYPE_INT_ARGB expects and
    // every pixel is one masked table lookup.
    private static final class NesEngine {
        final Nes nes;
        final Memory memory;
        final Rt.Fn setInputFn;
        final Rt.Fn tickGameFn;
        final int screenOff;
        final int[] palette = new int[64];

        final int width;
        final int height;
        final BufferedImage frame;

        NesEngine(byte[] rom) {
            // This module has zero wasm imports (verified by nes.sh with
            // wasm-objdump) and no WASI imports either — null is tolerated
            // for the imports map and for args/env/preopens.
            this.nes = new Nes(null, null, null, null);
            this.memory = (Memory) nes.Exports.get("memory");

            Rt.Fn allocRomFn = (Rt.Fn) nes.Exports.get("allocRom");
            Rt.Fn initGameFn = (Rt.Fn) nes.Exports.get("initGame");
            this.setInputFn = (Rt.Fn) nes.Exports.get("setInput");
            this.tickGameFn = (Rt.Fn) nes.Exports.get("tickGame");
            Rt.Fn screenOffsetFn = (Rt.Fn) nes.Exports.get("screenOffset");
            Rt.Fn paletteOffsetFn = (Rt.Fn) nes.Exports.get("paletteOffset");
            Rt.Fn frameWidthFn = (Rt.Fn) nes.Exports.get("frameWidth");
            Rt.Fn frameHeightFn = (Rt.Fn) nes.Exports.get("frameHeight");

            int ptr = (Integer) allocRomFn.invoke(new Object[] { rom.length });
            memory.init(Integer.toUnsignedLong(ptr), rom, 0, rom.length);

            int ok = (Integer) initGameFn.invoke(new Object[0]);
            if (ok != 1) {
                throw new IllegalStateException("initGame failed (bad or unsupported ROM)");
            }

            this.width = (Integer) frameWidthFn.invoke(new Object[0]);
            this.height = (Integer) frameHeightFn.invoke(new Object[0]);
            this.frame = new BufferedImage(width, height, BufferedImage.TYPE_INT_ARGB);

            // Both offsets are stable for the emulator's lifetime; the palette
            // is fixed data (R,G,B,A, alpha padding), so it is decoded once.
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
                // One byte per pixel, a palette index; the & 0x3f mask is
                // load-bearing (see examples/apps/src/nes_demo.c).
                pixels[i] = palette[d[screenOff + i] & 0x3f];
            }
        }
    }
}
