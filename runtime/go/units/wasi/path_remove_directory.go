// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
func (w *WASI) wasi_path_remove_directory(dirfd, pathPtr, pathLen uint32) uint32 {
    rel := string(w.memory.read_string(uint64(pathPtr), uint64(pathLen)))
    // rmdir(2) never follows a trailing symlink; use the raw syscall so it
    // fails (ENOTDIR) on a non-directory rather than unlinking it.
    hostPath, err := w.resolve_path(dirfd, rel, false)
    if err != wasiOk {
        return err
    }
    // wasmtime 47 (ADR-49) rejects removing an existing directory through a
    // slash-suffixed name with EINVAL on both hosts (cap-std's final-component
    // handling); a missing target stays ENOENT (the syscall below) and a
    // non-directory was already ENOTDIR in resolve_path.
    if strings.HasSuffix(rel, "/") {
        if fi, e := os.Stat(hostPath); e == nil && fi.IsDir() {
            return wasiInval
        }
    }
    if e := syscall.Rmdir(hostPath); e != nil {
        return w.fs_errno(e)
    }
    return wasiOk
}
