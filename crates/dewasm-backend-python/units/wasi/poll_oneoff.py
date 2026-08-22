# requires: memory/fill, memory/iwl, memory/uwlb, memory/uwlh, memory/idl, memory/iws, memory/iwsb, memory/iwsh, memory/ids
# poll_oneoff waits until at least one subscription is ready, then writes one event per ready subscription (WASI p1 layout: 48-byte subscriptions in,
# 32-byte events out).
# Only fd_read on stdin actually blocks (via select.select); regular files, stdout/stderr, and every fd_write are treated as immediately ready, and unknown fds report EBADF.
# Clock subscriptions set the wait deadline; if it elapses with no fd ready, the due clock subs fire.
# Motivated by event-loop guests such as the QuickJS REPL, which blocks here on stdin between prompts.
def wasi_poll_oneoff(self, in_ptr, out_ptr, nsubs, nevents_ptr):
    if nsubs == 0:
        return self.ERRNO_INVAL
    ready = []    # (userdata, error, type, nbytes, flags) resolvable without waiting
    waiters = []  # (userdata, type, io) fd_read on stdin: needs a host wait
    clocks = []   # (userdata, rel_ns)
    for i in range(nsubs):
        base = in_ptr + i * 48
        userdata = self.memory.idl(base)
        tag = self.memory.uwlb(base + 8)
        if tag == 0:  # clock
            clock_id = self.memory.iwl(base + 16)
            timeout = self.memory.idl(base + 24)
            flags = self.memory.uwlh(base + 40)
            now = time.time_ns() if clock_id == 0 else time.monotonic_ns()
            rel = max(timeout - now, 0) if (flags & 1) else timeout
            clocks.append((userdata, rel))
        elif tag in (1, 2):  # fd_read / fd_write
            fd = self.memory.iwl(base + 16)
            io = self.fds.get(fd)
            if io is None or isinstance(io, self.WasiDir):
                ready.append((userdata, self.ERRNO_BADF, tag, 0, 0))
            elif tag == 1 and io is self.std_ios[0]:
                waiters.append((userdata, tag, io))
            else:
                nbytes = 1
                if tag == 1:
                    try:
                        nbytes = max(os.fstat(io.fileno()).st_size - io.tell(), 0)
                    except OSError:
                        nbytes = 1
                ready.append((userdata, 0, tag, nbytes, 0))
        else:
            return self.ERRNO_INVAL

    events = ready
    if not events:
        if waiters:
            timeout_s = None if not clocks else min(c[1] for c in clocks) / 1e9
            readable, _, _ = select.select([w[2] for w in waiters], [], [], timeout_s)
            for userdata, type_, io in waiters:
                if io in readable:
                    events.append((userdata, 0, type_, 1, 0))
        elif clocks:
            time.sleep(min(c[1] for c in clocks) / 1e9)
        if not events and clocks:
            due = min(c[1] for c in clocks)
            for userdata, rel in clocks:
                if rel <= due:
                    events.append((userdata, 0, 0, 0, 0))

    for i, (userdata, error, type_, nbytes, flags) in enumerate(events):
        ev = out_ptr + i * 32
        self.memory.fill(ev, 0, 32)
        self.memory.ids(ev, userdata)
        self.memory.iwsh(ev + 8, error)
        self.memory.iwsb(ev + 10, type_)
        self.memory.ids(ev + 16, nbytes)
        self.memory.iwsh(ev + 24, flags)
    self.memory.iws(nevents_ptr, len(events))
    return self.ERRNO_SUCCESS
