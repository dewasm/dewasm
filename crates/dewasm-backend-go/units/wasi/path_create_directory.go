// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
func (w *WASI) wasi_path_create_directory(dirfd, pathPtr, pathLen uint32) uint32 {
    rel := string(w.memory.read_string(uint64(pathPtr), uint64(pathLen)))
    // Strip a trailing slash before the resolver's directory check: mkdir names a directory anyway, and EEXIST is wasmtime's answer for mkdir("file/") where the hosts split.
    if trimmed := strings.TrimRight(rel, "/"); trimmed != "" {
        rel = trimmed
    }
    // mkdir(2) never follows a trailing symlink (an existing one is EEXIST).
    hostPath, err := w.resolve_path(dirfd, rel, false)
    if err != wasiOk {
        return err
    }
    if e := os.Mkdir(hostPath, 0o755); e != nil {
        return w.fs_errno(e)
    }
    return wasiOk
}
