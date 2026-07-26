// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
int wasi_path_unlink_file(int dirfd, int pathPtr, int pathLen) {
    String rel = new String(
        memory.read_string(Integer.toUnsignedLong(pathPtr), Integer.toUnsignedLong(pathLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    // unlink(2) never follows a trailing symlink: it removes the link itself.
    Resolved r = resolve_path(dirfd, rel, false);
    if (r.errno != WASI_OK) {
        return r.errno;
    }
    java.nio.file.Path p = java.nio.file.Paths.get(r.path);
    // Files.delete would remove an empty directory; unlink must fail (EISDIR)
    // on any directory, so pre-check.
    if (java.nio.file.Files.isDirectory(p, java.nio.file.LinkOption.NOFOLLOW_LINKS)) {
        return WASI_ISDIR;
    }
    try {
        java.nio.file.Files.delete(p);
    } catch (java.io.IOException ex) {
        return fs_errno(ex);
    }
    return WASI_OK;
}
