// fd_allocate grows the file so at least offset+len bytes are backed; it never shrinks (a request within the current size is a no-op).
// Emulated with
// Truncate, which is enough for the guests we target.
func (w *WASI) wasi_fd_allocate(fd uint32, offset, length uint64) uint32 {
    f, ok := w.fds[fd].(*os.File)
    if !ok {
        return wasiBadf
    }
    fi, err := f.Stat()
    if err != nil {
        return wasiIo
    }
    if need := int64(offset + length); need > fi.Size() {
        if err := f.Truncate(need); err != nil {
            return wasiIo
        }
    }
    return wasiOk
}
