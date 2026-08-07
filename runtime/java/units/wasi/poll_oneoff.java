// requires: memory/fill, memory/i32_load, memory/i32_load8_u, memory/i32_load16_u, memory/i64_load, memory/i32_store, memory/i32_store8, memory/i32_store16, memory/i64_store
// poll_oneoff waits until at least one subscription is ready, then writes one
// event per ready subscription (WASI p1 layout: 48-byte subscriptions in,
// 32-byte events out). Regular files, stdout/stderr, and every fd_write are
// treated as immediately ready, and unknown fds report EBADF. Clock
// subscriptions set the wait deadline; if it elapses with no fd ready, the due
// clock subs fire. Motivated by event-loop guests such as the QuickJS REPL,
// which blocks here on stdin between prompts.
//
// Java limitation: there is no select(2) on System.in, so an fd_read wait on
// stdin is approximated by polling InputStream.available() with a 1 ms sleep.
// A canonical-mode tty reports 0 until a whole line is entered, and EOF is
// indistinguishable from "no data yet", so a no-clock (infinite) wait blocks
// until bytes actually arrive. Adequate for the event-loop use case; byte-exact
// interactive behaviour is out of scope for this approximation.
int wasi_poll_oneoff(int inPtr, int outPtr, int nsubs, int neventsPtr) {
    if (nsubs == 0) {
        return WASI_INVAL;
    }
    // Each event row: {userdata, error, type, nbytes, flags}.
    java.util.List<long[]> ready = new java.util.ArrayList<>();
    java.util.List<long[]> stdinWaiters = new java.util.ArrayList<>(); // {userdata, type}
    java.util.List<long[]> clocks = new java.util.ArrayList<>(); // {userdata, rel_ns}
    for (int i = 0; i < nsubs; i++) {
        long base = Integer.toUnsignedLong(inPtr) + (long) i * 48;
        long userdata = memory.i64_load(base);
        int tag = memory.i32_load8_u(base + 8);
        if (tag == 0) { // clock
            long timeout = memory.i64_load(base + 24);
            int flags = memory.i32_load16_u(base + 40);
            long rel;
            if ((flags & 1) != 0) { // ABSTIME (realtime & monotonic both map to the wall clock, per clock_time_get)
                rel = timeout - System.currentTimeMillis() * 1_000_000L;
                if (rel < 0) {
                    rel = 0;
                }
            } else {
                rel = timeout;
            }
            clocks.add(new long[] {userdata, rel});
        } else if (tag == 1 || tag == 2) { // fd_read / fd_write
            int fd = memory.i32_load(base + 16);
            Object entry = fds.get(fd);
            if (entry == null || entry instanceof Dir) {
                ready.add(new long[] {userdata, WASI_BADF, tag, 0, 0});
            } else if (tag == 1 && entry == stdin) {
                stdinWaiters.add(new long[] {userdata, tag});
            } else {
                long nbytes = 1;
                if (tag == 1 && entry instanceof Handle) {
                    try {
                        java.nio.channels.FileChannel ch = ((Handle) entry).ch;
                        long avail = ch.size() - ch.position();
                        nbytes = avail > 0 ? avail : 1;
                    } catch (java.io.IOException ex) {
                        nbytes = 1;
                    }
                }
                ready.add(new long[] {userdata, 0, tag, nbytes, 0});
            }
        } else {
            return WASI_INVAL;
        }
    }

    long minRel = Long.MAX_VALUE;
    for (long[] c : clocks) {
        if (c[1] < minRel) {
            minRel = c[1];
        }
    }

    java.util.List<long[]> events = ready;
    if (events.isEmpty()) {
        if (!stdinWaiters.isEmpty()) {
            long deadline = clocks.isEmpty() ? -1 : System.nanoTime() + minRel;
            boolean readable = false;
            try {
                while (true) {
                    if (stdin.available() > 0) {
                        readable = true;
                        break;
                    }
                    if (deadline >= 0 && System.nanoTime() >= deadline) {
                        break;
                    }
                    Thread.sleep(1);
                }
            } catch (Exception ex) {
                // Fall through with whatever readiness we have.
            }
            if (readable) {
                for (long[] wsub : stdinWaiters) {
                    events.add(new long[] {wsub[0], 0, wsub[1], 1, 0});
                }
            }
        } else if (!clocks.isEmpty()) {
            try {
                Thread.sleep(minRel / 1_000_000L, (int) (minRel % 1_000_000L));
            } catch (InterruptedException ex) {
                // Treated as elapsed.
            }
        }
        if (events.isEmpty() && !clocks.isEmpty()) {
            for (long[] c : clocks) {
                if (c[1] <= minRel) {
                    events.add(new long[] {c[0], 0, 0, 0, 0});
                }
            }
        }
    }

    for (int i = 0; i < events.size(); i++) {
        long[] e = events.get(i);
        long base = Integer.toUnsignedLong(outPtr) + (long) i * 32;
        memory.fill(base, 0, 32);
        memory.i64_store(base, e[0]);
        memory.i32_store16(base + 8, (int) e[1]);
        memory.i32_store8(base + 10, (int) e[2]);
        memory.i64_store(base + 16, e[3]);
        memory.i32_store16(base + 24, (int) e[4]);
    }
    memory.i32_store(Integer.toUnsignedLong(neventsPtr), events.size());
    return WASI_OK;
}
