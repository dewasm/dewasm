// Map a file's basic attributes to a WASI filetype tag. Java's portable
// BasicFileAttributes only distinguishes directory / symlink / regular / other,
// so — unlike the Go backend, which reads FileMode bits — block/character
// devices and sockets both collapse to "unknown" (0) here. That is adequate
// for the files and directories our guests touch; tty detection for the
// standard streams is handled separately in fd_fdstat_get (ADR-30).
byte wasi_filetype(java.nio.file.attribute.BasicFileAttributes a) {
    if (a.isDirectory()) {
        return 3; // directory
    }
    if (a.isSymbolicLink()) {
        return 7; // symbolic link
    }
    if (a.isRegularFile()) {
        return 4; // regular file
    }
    return 0; // unknown
}
