// requires: memory/i32_load, memory/i32_store, memory/init
func (w *WASI) wasi_fd_read(fd, iovsPtr, iovsLen, nreadPtr uint32) uint32 {
    f := w.fds[fd]
    if f == nil {
        return wasiBadf
    }
    nread := uint32(0)
    for i := uint32(0); i < iovsLen; i++ {
        ptr := w.memory.i32_load(uint64(iovsPtr) + uint64(i)*8)
        length := w.memory.i32_load(uint64(iovsPtr)+uint64(i)*8 + 4)
        if length == 0 {
            continue
        }
        buf := make([]byte, length)
        n, err := f.Read(buf)
        if n > 0 {
            w.memory.init(uint64(ptr), buf, 0, uint64(n))
            nread += uint32(n)
        }
        if err != nil || uint32(n) < length {
            break
        }
    }
    w.memory.i32_store(uint64(nreadPtr), nread)
    return wasiOk
}
