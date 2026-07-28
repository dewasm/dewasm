// requires: memory/read_string, wasi/resolve_path, wasi/resolve_times, wasi/errno_fs
// Set the access/modification times of a path (ADR-40). SYMLINK_FOLLOW selects
// whether the symlink or its target is affected; os.Chtimes itself follows
// symlinks, so the NOFOLLOW-on-a-symlink case (no std lutimes) is a known gap.
func (w *WASI) wasi_path_filestat_set_times(fd, flags, pathPtr, pathLen uint32, atim, mtim uint64, fstflags uint32) uint32 {
    rel := string(w.memory.read_string(uint64(pathPtr), uint64(pathLen)))
    hostPath, err := w.resolve_path(fd, rel, flags&0x1 != 0) // lookupflags::SYMLINK_FOLLOW
    if err != wasiOk {
        return err
    }
    atime, mtime, e := w.resolve_times(atim, mtim, fstflags)
    if e != wasiOk {
        return e
    }
    if err := os.Chtimes(hostPath, atime, mtime); err != nil {
        return w.fs_errno(err)
    }
    return wasiOk
}
