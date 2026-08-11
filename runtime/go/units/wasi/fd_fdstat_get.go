// requires: memory/fill, memory/i32_store8, memory/i32_store16, memory/i64_store
func (w *WASI) wasi_fd_fdstat_get(fd, outPtr uint32) uint32 {
    entry, present := w.fds[fd]
    if !present {
        return wasiBadf
    }
    var filetype uint32 = 4 // regular file
    if _, ok := entry.(*wasiDir); ok {
        filetype = 3 // directory
    } else if f, ok := entry.(*os.File); ok {
        if fi, err := f.Stat(); err == nil && fi.Mode()&os.ModeCharDevice != 0 {
            filetype = 2 // character device (tty)
        }
    }
    // fdstat: fs_filetype (u8) + pad + fs_flags (u16) + pad + fs_rights_base
    // (u64) + fs_rights_inheriting (u64) = 24 bytes.
    // Rights and fdflags come from the stored per-fd meta; a missing entry (should not happen for a live fd) reports the permissive all-ones.
    base, inheriting := ^uint64(0), ^uint64(0)
    var fdflags uint16
    if m, ok := w.meta[fd]; ok {
        base, inheriting, fdflags = m.base, m.inheriting, m.fdflags
    }
    w.memory.fill(uint64(outPtr), 0, 24)
    w.memory.i32_store8(uint64(outPtr), filetype)
    w.memory.i32_store16(uint64(outPtr)+2, uint32(fdflags))
    w.memory.i64_store(uint64(outPtr)+8, base)
    w.memory.i64_store(uint64(outPtr)+16, inheriting)
    return wasiOk
}
