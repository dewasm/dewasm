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
    // A slash-suffixed destination may only name an existing directory or one
    // the rename itself creates from a directory source (POSIX pathname
    // resolution; issue #42). resolve_path already reported ENOTDIR for an
    // existing non-directory; the nonexistent case must not fall through — the
    // resolved host path has lost the slash, so rename(2) would silently
    // create a plain *file* at the name. ENOENT is the macOS/POSIX errno
    // (Linux would say ENOTDIR), the choice the other backends normalize to
    // (runtime/bash/units/wasi/path_rename.sh).
    if strings.HasSuffix(newRel, "/") {
        if _, e := os.Lstat(newHost); e != nil {
            if fi, se := os.Lstat(oldHost); se != nil || !fi.IsDir() {
                return wasiNoent
            }
        }
    }
    // syscall.Rename, not os.Rename: Go's os.Rename wrapper Lstats the
    // destination and, when it is a directory, returns a synthetic EEXIST on
    // macOS instead of letting rename(2) replace an empty target dir — the
    // atomic dir-onto-empty-dir semantics the suite requires. The raw syscall
    // has the correct POSIX behaviour (ENOTEMPTY on a non-empty target,
    // EISDIR/ENOTDIR on type mismatches) (ADR-40).
    if e := syscall.Rename(oldHost, newHost); e != nil {
        return w.fs_errno(e)
    }
    return wasiOk
}
