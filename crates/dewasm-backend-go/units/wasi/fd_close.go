func (w *WASI) wasi_fd_close(fd uint32) uint32 {
    entry, present := w.fds[fd]
    if !present {
        return wasiBadf
    }
    delete(w.fds, fd)
    if f, ok := entry.(*os.File); ok && !w.isStdio(f) {
        f.Close()
    }
    // Fds are never reused after close; a *wasiDir has no OS handle.
    return wasiOk
}
