func (w *WASI) wasi_fd_sync(fd uint32) uint32 {
    f, ok := w.fds[fd].(*os.File)
    if !ok {
        return wasiBadf
    }
    if err := f.Sync(); err != nil {
        return wasiIo
    }
    return wasiOk
}
