// requires: memory/i64_store
func (w *WASI) wasi_fd_seek(fd uint32, offset uint64, whence, outPtr uint32) uint32 {
    f, ok := w.fds[fd].(*os.File)
    if !ok {
        return wasiBadf
    }
    if w.isStdio(f) {
        return wasiSpipe
    }
    // WASI whence (0=set, 1=cur, 2=end) matches Go's io.Seek* values exactly.
    if whence > 2 {
        return wasiInval
    }
    pos, err := f.Seek(int64(offset), int(whence))
    if err != nil {
        return wasiIo
    }
    w.memory.i64_store(uint64(outPtr), uint64(pos))
    return wasiOk
}
