// requires: memory/i32_load, memory/i32_store, memory/read_string
func (w *WASI) wasi_fd_pwrite(fd, iovsPtr, iovsLen uint32, offset uint64, nwrittenPtr uint32) uint32 {
    f, ok := w.fds[fd].(*os.File)
    if !ok {
        return wasiBadf
    }
    if w.isStdio(f) {
        return wasiSpipe
    }
    if e := w.checkRight(fd, rightFdWrite); e != wasiOk {
        return e
    }
    written := uint32(0)
    for i := uint32(0); i < iovsLen; i++ {
        ptr := w.memory.i32_load(uint64(iovsPtr) + uint64(i)*8)
        length := w.memory.i32_load(uint64(iovsPtr)+uint64(i)*8 + 4)
        chunk := w.memory.read_string(uint64(ptr), uint64(length))
        // syscall.Pwrite rather than f.WriteAt: WriteAt rejects a fd opened
        // O_APPEND ("invalid use of WriteAt"), and a positional write must ignore append anyway.
        // Portable on darwin+linux.
        n, err := syscall.Pwrite(int(f.Fd()), chunk, int64(offset+uint64(written)))
        written += uint32(n)
        if err != nil {
            return wasiIo
        }
    }
    w.memory.i32_store(uint64(nwrittenPtr), written)
    return wasiOk
}
