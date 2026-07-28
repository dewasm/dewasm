// Set the atim/mtim of an open fd (ADR-40). fst_flags selects which of atim and
// mtim to set and whether to "now"; setting a timestamp both explicitly and to
// "now" is EINVAL. A null FileTime leaves that timestamp untouched, so a guest
// can change one without disturbing the other.
int wasi_fd_filestat_set_times(int fd, long atim, long mtim, int fstflags) {
    Object e = fds.get(fd);
    java.nio.file.Path p;
    if (e instanceof Handle) {
        p = ((Handle) e).path;
    } else if (e instanceof Dir) {
        p = ((Dir) e).hostPath;
    } else {
        return WASI_BADF;
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
    try {
        java.nio.file.Files.getFileAttributeView(
            p, java.nio.file.attribute.BasicFileAttributeView.class).setTimes(mt, at, null);
    } catch (java.io.IOException ex) {
        return WASI_IO;
    }
    return WASI_OK;
}
