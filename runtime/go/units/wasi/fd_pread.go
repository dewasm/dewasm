// requires: memory/i32_load, memory/i32_store, memory/init
func (w *WASI) wasi_fd_pread(fd, iovsPtr, iovsLen uint32, offset uint64, nreadPtr uint32) uint32 {
    f, ok := w.fds[fd].(*os.File)
    if !ok {
        return wasiBadf
    }
    if w.isStdio(f) {
        return wasiSpipe
    }
    nread := uint32(0)
    for i := uint32(0); i < iovsLen; i++ {
        ptr := w.memory.i32_load(uint64(iovsPtr) + uint64(i)*8)
        length := w.memory.i32_load(uint64(iovsPtr)+uint64(i)*8 + 4)
        if length == 0 {
            continue
        }
        buf := make([]byte, length)
        n, err := f.ReadAt(buf, int64(offset+uint64(nread)))
        if n > 0 {
            w.memory.init(uint64(ptr), buf, 0, uint64(n))
            nread += uint32(n)
        }
        // ReadAt returns io.EOF (a non-nil err) on a short read at end of
        // file; either a short read or any error ends the scatter read.
        if err != nil || uint32(n) < length {
            break
        }
    }
    w.memory.i32_store(uint64(nreadPtr), nread)
    return wasiOk
}
