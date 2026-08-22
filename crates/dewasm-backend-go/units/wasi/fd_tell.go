// requires: memory/i64_store
func (w *WASI) wasi_fd_tell(fd, outPtr uint32) uint32 {
    f, ok := w.fds[fd].(*os.File)
    if !ok {
        return wasiBadf
    }
    if w.isStdio(f) {
        return wasiSpipe
    }
    pos, err := f.Seek(0, 1) // whence 1 = SEEK_CUR
    if err != nil {
        return wasiIo
    }
    w.memory.i64_store(uint64(outPtr), uint64(pos))
    return wasiOk
}
