// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
// Both endpoints are resolved NOFOLLOW, and LOOKUPFLAGS_SYMLINK_FOLLOW is rejected as EINVAL like the other backends.
func (w *WASI) wasi_path_link(oldDirfd, oldFlags, oldPathPtr, oldPathLen, newDirfd, newPathPtr, newPathLen uint32) uint32 {
    if oldFlags&0x1 != 0 { // lookupflags::SYMLINK_FOLLOW
        return wasiInval
    }
    oldRel := string(w.memory.read_string(uint64(oldPathPtr), uint64(oldPathLen)))
    oldHost, err := w.resolve_path(oldDirfd, oldRel, false)
    if err != wasiOk {
        return err
    }
    newRel := string(w.memory.read_string(uint64(newPathPtr), uint64(newPathLen)))
    // A trailing slash on the new name is NOENT: the link is a fresh leaf.
    if len(newRel) > 0 && newRel[len(newRel)-1] == '/' {
        return wasiNoent
    }
    newHost, err := w.resolve_path(newDirfd, newRel, false)
    if err != wasiOk {
        return err
    }
    if e := os.Link(oldHost, newHost); e != nil {
        // macOS link(2) follows a symlink source (unlike Linux and unlike the
        // AT_SYMLINK_NOFOLLOW linkat the suite expects), and std exposes no portable linkat.
        // Emulate a NOFOLLOW hard-link-to-a-symlink by recreating the symlink at the destination.
        if fi, le := os.Lstat(oldHost); le == nil && fi.Mode()&os.ModeSymlink != 0 {
            if target, re := os.Readlink(oldHost); re == nil {
                if se := os.Symlink(target, newHost); se != nil {
                    return w.fs_errno(se)
                }
                return wasiOk
            }
        }
        return w.fs_errno(e)
    }
    return wasiOk
}
