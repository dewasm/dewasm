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
    // A missing slash-suffixed target is ENOENT, not ENOTDIR: resolve_path's
    // directory check only rejects *existing* non-directories (issue #42).
    if (rel.endsWith("/")
        && !java.nio.file.Files.exists(p, java.nio.file.LinkOption.NOFOLLOW_LINKS)) {
        return WASI_NOENT;
    }
    // Files.delete would remove an empty directory; unlink must fail on any
    // directory. Errno per host, as wasmtime inherits it (ADR-49): EPERM on
    // macOS, EISDIR on Linux.
    if (java.nio.file.Files.isDirectory(p, java.nio.file.LinkOption.NOFOLLOW_LINKS)) {
        return System.getProperty("os.name").toLowerCase().contains("mac")
            ? WASI_PERM
            : WASI_ISDIR;
    }
    // A trailing slash demands a directory; on a non-directory target that is
    // ENOTDIR (a plain unlink of the file without the slash still succeeds).
    if (rel.endsWith("/")) {
        return WASI_NOTDIR;
    }
    try {
        java.nio.file.Files.delete(p);
    } catch (java.io.IOException ex) {
        return fs_errno(ex);
    }
    return WASI_OK;
}
