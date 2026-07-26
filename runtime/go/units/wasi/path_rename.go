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
    if e := os.Rename(oldHost, newHost); e != nil {
        return w.fs_errno(e)
    }
    return wasiOk
}
