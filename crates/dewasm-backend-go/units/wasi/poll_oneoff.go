// requires: memory/fill, memory/i32_load, memory/i32_load8_u, memory/i32_load16_u, memory/i64_load, memory/i32_store, memory/i32_store8, memory/i32_store16, memory/i64_store
// poll_oneoff waits until at least one subscription is ready, then writes one event per ready subscription (WASI p1 layout: 48-byte subscriptions in,
// 32-byte events out).
// Only fd_read on stdin actually blocks (via syscall.Select); regular files, stdout/stderr, and every fd_write are treated as immediately ready, and unknown fds report EBADF.
// Clock subscriptions set the wait deadline; if it elapses with no fd ready, the due clock subs fire.
// Motivated by event-loop guests such as the QuickJS REPL, which blocks here on stdin between prompts.
func (w *WASI) wasi_poll_oneoff(inPtr, outPtr, nsubs, neventsPtr uint32) uint32 {
    if nsubs == 0 {
        return wasiInval
    }
    type event struct {
        userdata uint64
        errno    uint32
        typ      uint32
        nbytes   uint64
    }
    type waiter struct {
        userdata uint64
        typ      uint32
        fd       int
    }
    var ready []event
    var waiters []waiter
    var clockRel []int64
    var clockUserdata []uint64
    for i := uint32(0); i < nsubs; i++ {
        base := uint64(inPtr) + uint64(i)*48
        userdata := w.memory.i64_load(base)
        tag := w.memory.i32_load8_u(base + 8)
        switch tag {
        case 0: // clock
            timeout := w.memory.i64_load(base + 24)
            flags := w.memory.i32_load16_u(base + 40)
            var rel int64
            if flags&1 != 0 { // ABSTIME (realtime & monotonic both map to the wall clock, per clock_time_get)
                rel = int64(timeout) - time.Now().UnixNano()
                if rel < 0 {
                    rel = 0
                }
            } else {
                rel = int64(timeout)
            }
            clockRel = append(clockRel, rel)
            clockUserdata = append(clockUserdata, userdata)
        case 1, 2: // fd_read / fd_write
            fd := w.memory.i32_load(base + 16)
            f, isFile := w.fds[fd].(*os.File)
            if !isFile {
                ready = append(ready, event{userdata, wasiBadf, tag, 0})
            } else if tag == 1 && f == os.Stdin {
                waiters = append(waiters, waiter{userdata, tag, int(f.Fd())})
            } else {
                nbytes := uint64(1)
                if tag == 1 {
                    if info, err := f.Stat(); err == nil {
                        if pos, err := f.Seek(0, 1); err == nil && info.Size()-pos > 0 {
                            nbytes = uint64(info.Size() - pos)
                        }
                    }
                }
                ready = append(ready, event{userdata, wasiOk, tag, nbytes})
            }
        default:
            return wasiInval
        }
    }

    events := ready
    if len(events) == 0 {
        if len(waiters) > 0 {
            var rset syscall.FdSet
            // FdSet is a bit array; on the little-endian targets we support bit n lives in byte n/8 regardless of the platform word width (int32 on darwin, int64 on linux), so address it byte-wise.
            bits := (*[128]byte)(unsafe.Pointer(&rset))
            maxFd := 0
            for _, wt := range waiters {
                bits[wt.fd/8] |= 1 << (uint(wt.fd) % 8)
                if wt.fd > maxFd {
                    maxFd = wt.fd
                }
            }
            var tvp *syscall.Timeval
            if len(clockRel) > 0 {
                tv := syscall.NsecToTimeval(minInt64(clockRel))
                tvp = &tv
            }
            // syscall.Select's return signature differs by platform (linux:
            // (int, error); darwin: error), so call it as a statement and read readiness back from the set, which select clears for fds that did not fire.
            syscall.Select(maxFd+1, &rset, nil, nil, tvp)
            for _, wt := range waiters {
                if bits[wt.fd/8]&(1<<(uint(wt.fd)%8)) != 0 {
                    events = append(events, event{wt.userdata, wasiOk, wt.typ, 1})
                }
            }
        } else if len(clockRel) > 0 {
            time.Sleep(time.Duration(minInt64(clockRel)) * time.Nanosecond)
        }
        if len(events) == 0 && len(clockRel) > 0 {
            due := minInt64(clockRel)
            for i, rel := range clockRel {
                if rel <= due {
                    events = append(events, event{clockUserdata[i], wasiOk, 0, 0})
                }
            }
        }
    }

    for i, e := range events {
        base := uint64(outPtr) + uint64(i)*32
        w.memory.fill(base, 0, 32)
        w.memory.i64_store(base, e.userdata)
        w.memory.i32_store16(base+8, e.errno)
        w.memory.i32_store8(base+10, e.typ)
        w.memory.i64_store(base+16, e.nbytes)
        w.memory.i32_store16(base+24, 0)
    }
    w.memory.i32_store(uint64(neventsPtr), uint32(len(events)))
    return wasiOk
}

// minInt64 returns the smallest element of a non-empty slice.
func minInt64(xs []int64) int64 {
    m := xs[0]
    for _, x := range xs {
        if x < m {
            m = x
        }
    }
    return m
}
