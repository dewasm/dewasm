// requires: memory/read_string, memory/init, memory/i32_store, wasi/resolve_path, wasi/errno_fs
// Read the target of the symlink at (fd, path) into the guest buffer, returning
// the number of bytes written. readlink never follows the final component, and
// the result is truncated (not an error) to buf_len (ADR-40).
func (w *WASI) wasi_path_readlink(fd, pathPtr, pathLen, bufPtr, bufLen, bufusedPtr uint32) uint32 {
    rel := string(w.memory.read_string(uint64(pathPtr), uint64(pathLen)))
    hostPath, err := w.resolve_path(fd, rel, false)
    if err != wasiOk {
        return err
    }
    target, e := os.Readlink(hostPath)
    if e != nil {
        return w.fs_errno(e)
    }
    b := []byte(target)
    if uint32(len(b)) > bufLen {
        b = b[:bufLen]
    }
    w.memory.init(uint64(bufPtr), b, 0, uint64(len(b)))
    w.memory.i32_store(uint64(bufusedPtr), uint32(len(b)))
    return wasiOk
}
