// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
// Create a symlink at (fd, new_path) whose target is old_path, stored VERBATIM
// — never pre-resolved to a host path; containment is enforced later, when the
// link is followed (ADR-40). An absolute target cannot be represented within a
// preopen sandbox, so it is rejected (cap-std does the same; the suite requires
// "/" to fail). The link's own parent is resolved NOFOLLOW.
func (w *WASI) wasi_path_symlink(oldPathPtr, oldPathLen, fd, newPathPtr, newPathLen uint32) uint32 {
    target := string(w.memory.read_string(uint64(oldPathPtr), uint64(oldPathLen)))
    if strings.HasPrefix(target, "/") {
        return wasiNotcapable
    }
    newRel := string(w.memory.read_string(uint64(newPathPtr), uint64(newPathLen)))
    // A trailing slash on the link name is NOENT: the link is a fresh leaf.
    if len(newRel) > 0 && newRel[len(newRel)-1] == '/' {
        return wasiNoent
    }
    linkHost, err := w.resolve_path(fd, newRel, false)
    if err != wasiOk {
        return err
    }
    if e := os.Symlink(target, linkHost); e != nil {
        return w.fs_errno(e)
    }
    return wasiOk
}
