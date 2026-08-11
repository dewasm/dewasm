// requires: memory/i32_load, memory/i32_store, memory/read_string
func (w *WASI) wasi_fd_write(fd, iovsPtr, iovsLen, nwrittenPtr uint32) uint32 {
    f, ok := w.fds[fd].(*os.File)
    if !ok {
        return wasiBadf
    }
    if e := w.checkRight(fd, rightFdWrite); e != wasiOk {
        return e
    }
    // APPEND is honored here rather than by opening the OS fd O_APPEND, so
    // fd_fdstat_set_flags can turn it off at runtime: seek to end
    // before writing.
    if m, ok := w.meta[fd]; ok && m.fdflags&fdflagAppend != 0 && !w.isStdio(f) {
        f.Seek(0, 2) // whence 2 = io.SeekEnd
    }
    written := uint32(0)
    for i := uint32(0); i < iovsLen; i++ {
        ptr := w.memory.i32_load(uint64(iovsPtr) + uint64(i)*8)
        length := w.memory.i32_load(uint64(iovsPtr)+uint64(i)*8 + 4)
        n, err := f.Write(w.memory.read_string(uint64(ptr), uint64(length)))
        written += uint32(n)
        if err != nil {
            return wasiIo
        }
    }
    w.memory.i32_store(uint64(nwrittenPtr), written)
    return wasiOk
}
