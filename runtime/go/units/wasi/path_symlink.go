// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
// Create a symlink at (fd, new_path) whose target is old_path, stored VERBATIM, never pre-resolved to a host path; containment is enforced later, when the link is followed.
// An absolute target cannot be represented within a preopen sandbox, so it is rejected (cap-std does the same; the suite requires
// "/" to fail).
// The link's own parent is resolved NOFOLLOW.
func (w *WASI) wasi_path_symlink(oldPathPtr, oldPathLen, fd, newPathPtr, newPathLen uint32) uint32 {
    target := string(w.memory.read_string(uint64(oldPathPtr), uint64(oldPathLen)))
    if strings.HasPrefix(target, "/") {
        return wasiNotcapable
    }
    newRel := string(w.memory.read_string(uint64(newPathPtr), uint64(newPathLen)))
    linkHost, err := w.resolve_path(fd, newRel, false)
    if err != wasiOk {
        return err
    }
    // Slash-suffixed link name: EEXIST if something is there
    // (non-directories were ENOTDIR in resolve_path), else ENOENT.
    if strings.HasSuffix(newRel, "/") {
        if _, e := os.Lstat(linkHost); e == nil {
            return wasiExist
        }
        return wasiNoent
    }
    if e := os.Symlink(target, linkHost); e != nil {
        return w.fs_errno(e)
    }
    return wasiOk
}
