// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
func (w *WASI) wasi_path_rename(oldDirfd, oldPathPtr, oldPathLen, newDirfd, newPathPtr, newPathLen uint32) uint32 {
    oldRel := string(w.memory.read_string(uint64(oldPathPtr), uint64(oldPathLen)))
    // rename(2) never follows trailing symlinks: it moves the link itself and
    // replaces the destination link.
    oldHost, err := w.resolve_path(oldDirfd, oldRel, false)
    if err != wasiOk {
        return err
    }
    newRel := string(w.memory.read_string(uint64(newPathPtr), uint64(newPathLen)))
    newHost, err := w.resolve_path(newDirfd, newRel, false)
    if err != wasiOk {
        return err
    }
    // Trailing slashes (issue #42): existing non-directories were
    // ENOTDIR in resolve_path; a nonexistent slash-suffixed destination is
    // renamed bare, as wasmtime strips it — the resolved path already is.
    // syscall.Rename, not os.Rename: Go's os.Rename wrapper Lstats the
    // destination and, when it is a directory, returns a synthetic EEXIST on
    // macOS instead of letting rename(2) replace an empty target dir — the
    // atomic dir-onto-empty-dir semantics the suite requires. The raw syscall
    // has the correct POSIX behaviour (ENOTEMPTY on a non-empty target,
    // EISDIR/ENOTDIR on type mismatches).
    if e := syscall.Rename(oldHost, newHost); e != nil {
        return w.fs_errno(e)
    }
    return wasiOk
}
