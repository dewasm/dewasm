// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
// Set the atim/mtim of a path. lookupflags::SYMLINK_FOLLOW selects
// whether the times are set on a symlink itself or its target (NOFOLLOW view
// vs following). fst_flags validation matches fd_filestat_set_times: a
// timestamp set both explicitly and to "now" is EINVAL.
int wasi_path_filestat_set_times(int fd, int flags, int pathPtr, int pathLen, long atim, long mtim,
                                 int fstflags) {
    String rel = new String(
        memory.read_string(Integer.toUnsignedLong(pathPtr), Integer.toUnsignedLong(pathLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    boolean follow = (flags & 0x1) != 0; // lookupflags::SYMLINK_FOLLOW
    Resolved r = resolve_path(fd, rel, follow);
    if (r.errno != WASI_OK) {
        return r.errno;
    }
    boolean setAtim = (fstflags & 0x1) != 0; // fstflags::ATIM
    boolean atimNow = (fstflags & 0x2) != 0; // fstflags::ATIM_NOW
    boolean setMtim = (fstflags & 0x4) != 0; // fstflags::MTIM
    boolean mtimNow = (fstflags & 0x8) != 0; // fstflags::MTIM_NOW
    if ((setAtim && atimNow) || (setMtim && mtimNow)) {
        return WASI_INVAL;
    }
    java.nio.file.attribute.FileTime at = setAtim
        ? java.nio.file.attribute.FileTime.from(atim, java.util.concurrent.TimeUnit.NANOSECONDS)
        : (atimNow ? java.nio.file.attribute.FileTime.from(java.time.Instant.now()) : null);
    java.nio.file.attribute.FileTime mt = setMtim
        ? java.nio.file.attribute.FileTime.from(mtim, java.util.concurrent.TimeUnit.NANOSECONDS)
        : (mtimNow ? java.nio.file.attribute.FileTime.from(java.time.Instant.now()) : null);
    java.nio.file.LinkOption[] opts = follow
        ? new java.nio.file.LinkOption[0]
        : new java.nio.file.LinkOption[] {java.nio.file.LinkOption.NOFOLLOW_LINKS};
    try {
        java.nio.file.Files.getFileAttributeView(
            java.nio.file.Paths.get(r.path),
            java.nio.file.attribute.BasicFileAttributeView.class, opts).setTimes(mt, at, null);
    } catch (java.io.IOException ex) {
        return fs_errno(ex);
    }
    return WASI_OK;
}
