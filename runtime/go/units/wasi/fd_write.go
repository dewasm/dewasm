// requires: memory/i32_load, memory/i32_store, memory/read_string
func (w *WASI) wasi_fd_write(fd, iovsPtr, iovsLen, nwrittenPtr uint32) uint32 {
    f := w.fds[fd]
    if f == nil {
        return wasiBadf
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
