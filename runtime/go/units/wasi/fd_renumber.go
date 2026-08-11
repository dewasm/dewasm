// Both endpoints must be live descriptors, else BADF (matching the reference
// table.renumber). The descriptor being overwritten is released, except stdio,
// which must never be closed, and a preopen/dir, which has no OS handle.
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
