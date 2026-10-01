// Both endpoints must be live descriptors, else BADF (matching the reference table.renumber).
// The descriptor the renumber overwrites is released, with two exceptions.
// Stdio must never be closed, and a preopen or directory has no OS handle.
func (w *WASI) wasi_fd_renumber(from, to uint32) uint32 {
    fromEntry, ok := w.fds[from]
    if !ok {
        return wasiBadf
    }
    toEntry, ok := w.fds[to]
    if !ok {
        return wasiBadf
    }
    if f, isFile := toEntry.(*os.File); isFile && !w.isStdio(f) {
        f.Close()
    }
    w.fds[to] = fromEntry
    w.meta[to] = w.meta[from]
    delete(w.fds, from)
    delete(w.meta, from)
    return wasiOk
}
