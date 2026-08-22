// Narrow (never widen) the rights on an fd. supports_rights probes this by re-setting the current rights, which must succeed; a request adding any bit the fd does not already hold is NOTCAPABLE.
func (w *WASI) wasi_fd_fdstat_set_rights(fd uint32, base, inheriting uint64) uint32 {
    m, ok := w.meta[fd]
    if !ok {
        return wasiBadf
    }
    if base&^m.base != 0 || inheriting&^m.inheriting != 0 {
        return wasiNotcapable
    }
    m.base = base
    m.inheriting = inheriting
    return wasiOk
}
