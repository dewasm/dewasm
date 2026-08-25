// requires: memory/fill, memory/i32_store8, memory/i32_store16, memory/i64_store
func (w *WASI) wasi_fd_fdstat_get(fd, outPtr uint32) uint32 {
    entry, present := w.fds[fd]
    if !present {
        return wasiBadf
    }
    // fdstat: fs_filetype (u8) + pad + fs_flags (u16) + pad + fs_rights_base
    // (u64) + fs_rights_inheriting (u64) = 24 bytes.
    // Rights and fdflags come from the stored per-fd meta; a missing entry (should not happen for a live fd) reports the permissive all-ones.
    base, inheriting := ^uint64(0), ^uint64(0)
    var fdflags uint16
    m, hasMeta := w.meta[fd]
    if hasMeta {
        base, inheriting, fdflags = m.base, m.inheriting, m.fdflags
    }
    // The Stat behind the character-device answer is a host syscall, and an open descriptor's filetype cannot change while it is open, so it runs at most once per fd (see wasiFdMeta): a guest polling isatty in a loop would otherwise pay one syscall per call.
    var filetype uint32
    if hasMeta && m.filetypeKnown {
        filetype = m.filetype
    } else {
        filetype = 4 // regular file
        if _, ok := entry.(*wasiDir); ok {
            filetype = 3 // directory
        } else if f, ok := entry.(*os.File); ok {
            if fi, err := f.Stat(); err == nil && fi.Mode()&os.ModeCharDevice != 0 {
                filetype = 2 // character device (tty)
            }
        }
        if hasMeta {
            m.filetype, m.filetypeKnown = filetype, true
        }
    }
    w.memory.fill(uint64(outPtr), 0, 24)
    w.memory.i32_store8(uint64(outPtr), filetype)
    w.memory.i32_store16(uint64(outPtr)+2, uint32(fdflags))
    w.memory.i64_store(uint64(outPtr)+8, base)
    w.memory.i64_store(uint64(outPtr)+16, inheriting)
    return wasiOk
}
