// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
func (w *WASI) wasi_path_unlink_file(dirfd, pathPtr, pathLen uint32) uint32 {
    rel := string(w.memory.read_string(uint64(pathPtr), uint64(pathLen)))
    // unlink(2) never follows a trailing symlink: it removes the link.
    // The raw syscall fails (EISDIR/EPERM) on a directory rather than removing it.
    hostPath, err := w.resolve_path(dirfd, rel, false)
    if err != wasiOk {
        return err
    }
    // A trailing slash asks for a directory; unlink_file never removes one, so it is NOTDIR on a non-directory target.
    // A real directory falls through to the raw syscall, which fails EPERM/EISDIR.
    if len(rel) > 0 && rel[len(rel)-1] == '/' {
        if fi, e := os.Lstat(hostPath); e == nil && !fi.IsDir() {
            return wasiNotdir
        }
    }
    if e := syscall.Unlink(hostPath); e != nil {
        return w.fs_errno(e)
    }
    return wasiOk
}
