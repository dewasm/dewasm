// DOOM frontend for the dewasm-generated Doom.java library.
// It is in the default package, same as Doom.java.
// So Doom and its nested Doom.Rt/Doom.Global classes are reachable without an import.
// Two entry points share one engine: an interactive Swing window (default) and a smoke test.
// The smoke test (`--smoke`) is headless.
// It never touches java.awt.event/javax.swing, so it can run without a display.
//
// Everything the wasm host interface needs lives in DoomEngine.
// That is the imports map, save-file I/O, and the framebuffer copy.
// Main only connects it to either a window or a fixed tick count.

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
import java.nio.ByteBuffer;
import java.nio.ByteOrder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Map;
import java.util.Set;
import java.util.concurrent.ConcurrentLinkedQueue;
import javax.imageio.ImageIO;
import javax.swing.JFrame;
import javax.swing.JPanel;
import javax.swing.SwingUtilities;

public class Main {

    public static void main(String[] args) throws Exception {
        if (args.length > 0 && args[0].equals("--smoke")) {
            runSmoke();
        } else {
            runGui();
        }
    }

    // Headless self-test: `init()` plus a fixed number of ticks with no window.
    // It uses no KeyListener and no JFrame.
    // It uses only BufferedImage/ImageIO, which render in software and need no display.
    // A few Enter presses are injected along the way.
    // They drive DOOM from its title/legal screens through the menu defaults into a loaded level.
    // So the final frame is a real 3D view of a level, with lighting that varies per wall.
    // It is not a flat, low-color title card.
    // That's what the distinct-color sanity check below is actually probing for.
    private static void runSmoke() throws IOException {
        DoomEngine engine = new DoomEngine(null, null);
        engine.init();

        int enterKey = engine.keyMap.get("KEY_ENTER");
        int ticks = 300;
        long t0 = System.nanoTime();
        for (int i = 0; i < ticks; i++) {
            int phase = i % 15;
            if (phase == 5) {
                engine.reportKeyDown(enterKey);
            } else if (phase == 8) {
                engine.reportKeyUp(enterKey);
            }
            engine.tick();
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
        // DOOM's renderer shades a 256-entry palette through a few levels of reduced light.
        // So even a busy 3D scene tops out at a few hundred distinct colors per frame.
        // A first-level view with HUD, water, and one patch of blood measured ~150.
        // 1000 is unreachable by construction.
        // 100 is comfortably above a blank/solid-color buffer.
        // Such a buffer would mean the memory read was set up wrong.
        // 100 also stays below what any real rendered frame produces.
        if (colors.size() <= 100) {
            System.err.println("smoke: FAILED sanity check (expected > 100 distinct colors)");
            System.exit(1);
        }
        System.out.println("smoke: OK");
    }

    // Drawn on screen over the frame (mirrors `mapKey`).
    // That is because there's no other discoverability path for a window app.
    private static final String CONTROLS_TEXT =
        "arrows move  ctrl fire  space use  shift run  tab automap  ,/. strafe  1-7 weapon  esc menu";

    private static final String WINDOW_TITLE = "DOOM (dewasm)";

    // Interactive window: a JFrame whose panel draws the engine's current BufferedImage.
    // The image is scaled to the panel size.
    // A KeyListener puts key events on a queue for the game thread to drain.
    // DOOM ticks on its own dedicated thread, while Swing delivers input on the EDT.
    // The thread is dedicated since `reportKeyDown`/`Up`/`tickGame` are not thread-safe.
    // They must all be called from one thread.
    // The ConcurrentLinkedQueue passes the events from one to the other.
    private static void runGui() throws IOException {
        JFrame window = new JFrame(WINDOW_TITLE);
        DoomPanel panel = new DoomPanel();
        panel.controlsText = CONTROLS_TEXT;
        window.setContentPane(panel);
        window.setDefaultCloseOperation(JFrame.EXIT_ON_CLOSE);

        DoomEngine[] engineHolder = new DoomEngine[1];
        DoomEngine engine = new DoomEngine(
            () -> SwingUtilities.invokeLater(() -> {
                panel.setPreferredSize(new Dimension(engineHolder[0].width, engineHolder[0].height));
                window.pack();
                window.setLocationRelativeTo(null);
            }),
            panel::repaint);
        engineHolder[0] = engine;
        panel.engine = engine;

        window.addKeyListener(new KeyListener() {
            @Override
            public void keyTyped(KeyEvent e) {
            }

            @Override
            public void keyPressed(KeyEvent e) {
                Integer key = mapKey(engine, e);
                if (key != null) {
                    engine.queueKey(key, true);
                }
            }

            @Override
            public void keyReleased(KeyEvent e) {
                Integer key = mapKey(engine, e);
                if (key != null) {
                    engine.queueKey(key, false);
                }
            }
        });
        window.setFocusTraversalKeysEnabled(false);
        window.setFocusable(true);

        Thread gameThread = new Thread(() -> {
            engine.init();
            // FPS is measured over ~1s windows (tick counting), not one tick at a time.
            // A rate for one tick would be too noisy to read.
            // DOOM's own 35Hz pacing plus timing noise while the JIT warms up make it so.
            long fpsWindowStart = System.nanoTime();
            int fpsWindowTicks = 0;
            while (!Thread.currentThread().isInterrupted()) {
                engine.tick();
                fpsWindowTicks++;
                long now = System.nanoTime();
                double elapsedSec = (now - fpsWindowStart) / 1e9;
                if (elapsedSec >= 1.0) {
                    double fps = fpsWindowTicks / elapsedSec;
                    panel.fps = fps;
                    SwingUtilities.invokeLater(() -> window.setTitle(String.format("%s - %.1f FPS", WINDOW_TITLE, fps)));
                    fpsWindowStart = now;
                    fpsWindowTicks = 0;
                }
                try {
                    Thread.sleep(1);
                } catch (InterruptedException e) {
                    Thread.currentThread().interrupt();
                }
            }
        }, "doom-game-thread");
        gameThread.setDaemon(true);

        window.addWindowListener(new WindowAdapter() {
            @Override
            public void windowClosing(WindowEvent e) {
                gameThread.interrupt();
            }
        });

        window.setVisible(true);
        window.requestFocusInWindow();
        gameThread.start();
    }

    // Special keys route through the exported KEY_* globals.
    // Everything else is the ASCII value of the unmodified character in lower case.
    // That covers letters and digits.
    // DOOM reads those directly for menu text entry and weapon selection.
    // Returns null for keys with no DOOM mapping.
    private static Integer mapKey(DoomEngine engine, KeyEvent e) {
        switch (e.getKeyCode()) {
            case KeyEvent.VK_LEFT:
                return engine.keyMap.get("KEY_LEFTARROW");
            case KeyEvent.VK_RIGHT:
                return engine.keyMap.get("KEY_RIGHTARROW");
            case KeyEvent.VK_UP:
                return engine.keyMap.get("KEY_UPARROW");
            case KeyEvent.VK_DOWN:
                return engine.keyMap.get("KEY_DOWNARROW");
            case KeyEvent.VK_CONTROL:
                return engine.keyMap.get("KEY_FIRE");
            case KeyEvent.VK_SPACE:
                return engine.keyMap.get("KEY_USE");
            case KeyEvent.VK_SHIFT:
                return engine.keyMap.get("KEY_SHIFT");
            case KeyEvent.VK_TAB:
                return engine.keyMap.get("KEY_TAB");
            case KeyEvent.VK_ESCAPE:
                return engine.keyMap.get("KEY_ESCAPE");
            case KeyEvent.VK_ENTER:
                return engine.keyMap.get("KEY_ENTER");
            case KeyEvent.VK_BACK_SPACE:
                return engine.keyMap.get("KEY_BACKSPACE");
            case KeyEvent.VK_ALT:
                return engine.keyMap.get("KEY_ALT");
            case KeyEvent.VK_COMMA:
                return engine.keyMap.get("KEY_STRAFE_L");
            case KeyEvent.VK_PERIOD:
                return engine.keyMap.get("KEY_STRAFE_R");
            default:
                char c = Character.toLowerCase(e.getKeyChar());
                if (c >= 0x20 && c < 0x7f) {
                    return (int) c;
                }
                return null;
        }
    }

    // Draws the engine's current frame scaled to the panel, preserving aspect ratio.
    // Black bars fill the space left on the short axis.
    private static final class DoomPanel extends JPanel {
        volatile DoomEngine engine;
        volatile double fps;
        volatile String controlsText = "";

        DoomPanel() {
            setBackground(Color.BLACK);
            setFocusable(true);
        }

        @Override
        protected void paintComponent(Graphics g) {
            super.paintComponent(g);
            DoomEngine e = engine;
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

    // Connects the wasm host interface to the JVM.
    // Its parts are `console`, `gameSaving`, `runtimeControl`, `ui`, and `loading`.
    // It owns the Doom instance plus the current framebuffer.
    // The constructor's `onResize`/`onFrame` callbacks are null in the headless smoke path.
    // They are real callbacks in the GUI path.
    private static final class DoomEngine {
        final Doom doom;
        final Doom.Rt.Fn initGameFn;
        final Doom.Rt.Fn tickGameFn;
        final Doom.Rt.Fn reportKeyDownFn;
        final Doom.Rt.Fn reportKeyUpFn;
        final Map<String, Integer> keyMap = new HashMap<>();
        final ConcurrentLinkedQueue<int[]> keyEvents = new ConcurrentLinkedQueue<>();
        final long startNanos = System.nanoTime();
        final Runnable onResize;
        final Runnable onFrame;

        volatile int width;
        volatile int height;
        volatile BufferedImage frame;

        DoomEngine(Runnable onResize, Runnable onFrame) throws IOException {
            this.onResize = onResize;
            this.onFrame = onFrame;
            Files.createDirectories(Path.of(".savegame"));

            // The `Doom` constructor resolves imports during construction but never calls them.
            // So this holder lets the Doom.Rt.Fn lambdas below close over the instance.
            // That instance doesn't exist yet.
            // It's filled in immediately after construction.
            // It is read only from within `tick()`/`init()`, never during the constructor.
            Doom[] holder = new Doom[1];

            Doom.Rt.Fn onErrorMessage = a -> {
                logMessage(holder[0], (Integer) a[0], (Integer) a[1], System.err);
                return null;
            };
            Doom.Rt.Fn onInfoMessage = a -> {
                logMessage(holder[0], (Integer) a[0], (Integer) a[1], System.out);
                return null;
            };
            Doom.Rt.Fn sizeOfSaveGame = a -> (int) saveFile((Integer) a[0]).length();
            Doom.Rt.Fn readSaveGame = a -> {
                int id = (Integer) a[0];
                int dstOff = (Integer) a[1];
                File f = saveFile(id);
                if (!f.exists()) {
                    return 0;
                }
                byte[] bytes;
                try {
                    bytes = Files.readAllBytes(f.toPath());
                } catch (IOException e) {
                    throw new RuntimeException(e);
                }
                holder[0].memory.init(Integer.toUnsignedLong(dstOff), bytes, 0, bytes.length);
                return bytes.length;
            };
            Doom.Rt.Fn writeSaveGame = a -> {
                int id = (Integer) a[0];
                int srcOff = (Integer) a[1];
                int length = (Integer) a[2];
                byte[] buf = new byte[length];
                System.arraycopy(holder[0].memory.d, srcOff, buf, 0, length);
                try {
                    Files.write(saveFile(id).toPath(), buf);
                } catch (IOException e) {
                    throw new RuntimeException(e);
                }
                return length;
            };
            Doom.Rt.Fn timeInMilliseconds = a -> (System.nanoTime() - startNanos) / 1_000_000L;
            Doom.Rt.Fn drawFrame = a -> {
                int bufOff = (Integer) a[0];
                int[] pixels = ((DataBufferInt) frame.getRaster().getDataBuffer()).getData();
                ByteBuffer.wrap(holder[0].memory.d, bufOff, width * height * 4)
                    .order(ByteOrder.LITTLE_ENDIAN)
                    .asIntBuffer()
                    .get(pixels);
                if (onFrame != null) {
                    onFrame.run();
                }
                return null;
            };
            Doom.Rt.Fn onGameInit = a -> {
                width = (Integer) a[0];
                height = (Integer) a[1];
                // TYPE_INT_RGB, not ARGB.
                // The wasm framebuffer's top byte is not guaranteed to be 0xff.
                // An ARGB image would show whatever that byte happens to hold as alpha.
                // That could mean an alpha of 0 for every pixel.
                frame = new BufferedImage(width, height, BufferedImage.TYPE_INT_RGB);
                if (onResize != null) {
                    onResize.run();
                }
                return null;
            };
            Doom.Rt.Fn wadSizes = a -> null; // leave the pre-zeroed count/size in place: selects the embedded shareware WAD.
            Doom.Rt.Fn readWads = a -> null; // never called when wadSizes leaves the count at 0.

            Map<String, Map<String, Object>> imports = new HashMap<>();
            imports.computeIfAbsent("console", k -> new HashMap<>()).put("onErrorMessage", onErrorMessage);
            imports.computeIfAbsent("console", k -> new HashMap<>()).put("onInfoMessage", onInfoMessage);
            imports.computeIfAbsent("gameSaving", k -> new HashMap<>()).put("sizeOfSaveGame", sizeOfSaveGame);
            imports.computeIfAbsent("gameSaving", k -> new HashMap<>()).put("readSaveGame", readSaveGame);
            imports.computeIfAbsent("gameSaving", k -> new HashMap<>()).put("writeSaveGame", writeSaveGame);
            imports.computeIfAbsent("runtimeControl", k -> new HashMap<>()).put("timeInMilliseconds", timeInMilliseconds);
            imports.computeIfAbsent("ui", k -> new HashMap<>()).put("drawFrame", drawFrame);
            imports.computeIfAbsent("loading", k -> new HashMap<>()).put("wadSizes", wadSizes);
            imports.computeIfAbsent("loading", k -> new HashMap<>()).put("readWads", readWads);
            imports.computeIfAbsent("loading", k -> new HashMap<>()).put("onGameInit", onGameInit);

            // This module has no WASI imports.
            // The `Doom` constructor never reads arguments, environment variables, or preopens.
            // This was verified against the generated source.
            // So passing null for all three is safe.
            this.doom = new Doom(imports, null, null, null);
            holder[0] = doom;

            this.initGameFn = (Doom.Rt.Fn) doom.Exports.get("initGame");
            this.tickGameFn = (Doom.Rt.Fn) doom.Exports.get("tickGame");
            this.reportKeyDownFn = (Doom.Rt.Fn) doom.Exports.get("reportKeyDown");
            this.reportKeyUpFn = (Doom.Rt.Fn) doom.Exports.get("reportKeyUp");

            for (String name : new String[] {
                "KEY_LEFTARROW", "KEY_RIGHTARROW", "KEY_UPARROW", "KEY_DOWNARROW",
                "KEY_STRAFE_L", "KEY_STRAFE_R", "KEY_FIRE", "KEY_USE", "KEY_SHIFT",
                "KEY_TAB", "KEY_ESCAPE", "KEY_ENTER", "KEY_BACKSPACE", "KEY_ALT"
            }) {
                Doom.Global g = (Doom.Global) doom.Exports.get(name);
                keyMap.put(name, (Integer) g.value);
            }
        }

        private static File saveFile(int id) {
            return new File(".savegame/doomsav" + id + ".dsg");
        }

        private static void logMessage(Doom doom, int off, int len, Appendable out) {
            byte[] bytes = new byte[len];
            System.arraycopy(doom.memory.d, off, bytes, 0, len);
            String s = new String(bytes, StandardCharsets.UTF_8);
            try {
                // DOOM's messages carry no trailing newline.
                // The reference embedder prints one per message.
                out.append(s).append('\n');
            } catch (IOException e) {
                throw new RuntimeException(e);
            }
        }

        void init() {
            initGameFn.invoke(new Object[0]);
        }

        // Drains the key events on the queue (from the KeyListener, on the EDT) and ticks the game.
        // Both must happen on the same thread.
        // `reportKeyDown`/`Up` and `tickGame` are not thread-safe in the generated code.
        void tick() {
            int[] ev;
            while ((ev = keyEvents.poll()) != null) {
                if (ev[1] != 0) {
                    reportKeyDown(ev[0]);
                } else {
                    reportKeyUp(ev[0]);
                }
            }
            tickGameFn.invoke(new Object[0]);
        }

        void reportKeyDown(int key) {
            reportKeyDownFn.invoke(new Object[] { key });
        }

        void reportKeyUp(int key) {
            reportKeyUpFn.invoke(new Object[] { key });
        }

        void queueKey(int key, boolean down) {
            keyEvents.add(new int[] { key, down ? 1 : 0 });
        }
    }
}
