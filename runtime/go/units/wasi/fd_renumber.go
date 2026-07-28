// Renumber `from` onto `to`: both must be live descriptors (BADF otherwise,
// matching the reference table.renumber). The resource currently at `to` is
// released (a real file handle is closed; a preopen/dir has none, and stdio
// must never be closed), then `from`'s entry and its rights meta move to `to`
// and `from` is retired (ADR-40). Used to overwrite stdio and preopen slots.
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
