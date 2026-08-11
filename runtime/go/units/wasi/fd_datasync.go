// Go's os package exposes no fdatasync (and it is absent on macOS anyway), so
// fall back to a full fsync, a superset of the datasync guarantee (mirroring
// the Python backend's fallback).
func (w *WASI) wasi_fd_datasync(fd uint32) uint32 {
    f, ok := w.fds[fd].(*os.File)
    if !ok {
        return wasiBadf
    }
    if err := f.Sync(); err != nil {
        return wasiIo
    }
    return wasiOk
}
