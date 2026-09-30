// Map a file's basic attributes to a WASI `filetype` tag.
// Java's portable BasicFileAttributes only distinguishes directory/symbolic link/regular/other.
// So block/character devices and sockets both collapse to "unknown" (0) here.
// This differs from the Go backend, which reads FileMode bits.
// The collapse is adequate for the files and directories our guests touch.
// Terminal detection for the standard streams is handled separately in `fd_fdstat_get`.
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
