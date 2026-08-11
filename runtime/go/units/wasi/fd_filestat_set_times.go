// requires: wasi/resolve_times, wasi/errno_fs
// Set the access/modification times of a file fd. fstflags is
// validated first (so a bad combination is EINVAL even though nothing else
// runs); os.Chtimes leaves a field whose zero time.Time was returned untouched.
func (w *WASI) wasi_fd_filestat_set_times(fd uint32, atim, mtim uint64, fstflags uint32) uint32 {
    f, ok := w.fds[fd].(*os.File)
    if !ok {
        return wasiBadf
    }
    atime, mtime, e := w.resolve_times(atim, mtim, fstflags)
    if e != wasiOk {
        return e
    }
    if err := os.Chtimes(f.Name(), atime, mtime); err != nil {
        return w.fs_errno(err)
    }
    return wasiOk
}
