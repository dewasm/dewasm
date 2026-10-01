// Only APPEND is acted on (`fd_write` reads it back and seeks to end).
// Storing it here is what lets a guest turn append mode off at runtime.
// An O_APPEND OS handle could not.
// Any open `fd` is accepted, stdio included.
// `toywasm`'s WASI set-up sets NONBLOCK on `stdin` and treats failure as fatal.
// So Wasmtime's answer, EBADF for all but regular files, is not copied.
func (w *WASI) wasi_fd_fdstat_set_flags(fd, flags uint32) uint32 {
    m, ok := w.meta[fd]
    if !ok {
        return wasiBadf
    }
    m.fdflags = uint16(flags)
    return wasiOk
}
