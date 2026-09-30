// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
func (w *WASI) wasi_path_rename(oldDirfd, oldPathPtr, oldPathLen, newDirfd, newPathPtr, newPathLen uint32) uint32 {
    oldRel := string(w.memory.read_string(uint64(oldPathPtr), uint64(oldPathLen)))
    // `rename(2)` never follows trailing symbolic links.
    // It moves the link itself and replaces the destination link.
    oldHost, err := w.resolve_path(oldDirfd, oldRel, false)
    if err != wasiOk {
        return err
    }
    newRel := string(w.memory.read_string(uint64(newPathPtr), uint64(newPathLen)))
    newHost, err := w.resolve_path(newDirfd, newRel, false)
    if err != wasiOk {
        return err
    }
    // Trailing slashes (issue #42): existing non-directories were ENOTDIR in `resolve_path`.
    // A nonexistent slash-suffixed destination is renamed without the slash, as Wasmtime strips it.
    // The resolved path already has no slash.
    // syscall.Rename, not os.Rename: Go's os.Rename wrapper Lstats the destination.
    // When it is a directory, the wrapper returns a synthetic EEXIST on macOS.
    // It does not let `rename(2)` replace an empty target directory.
    // The suite requires those atomic semantics for a directory renamed onto an empty directory.
    // The raw system call has the correct POSIX behaviour.
    // That is ENOTEMPTY on a non-empty target, and EISDIR/ENOTDIR on type mismatches.
    if e := syscall.Rename(oldHost, newHost); e != nil {
        return w.fs_errno(e)
    }
    return wasiOk
}
