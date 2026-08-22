// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
int wasi_path_remove_directory(int dirfd, int pathPtr, int pathLen) {
    String rel = new String(
        memory.read_string(Integer.toUnsignedLong(pathPtr), Integer.toUnsignedLong(pathLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    // rmdir(2) never follows a trailing symlink.
    Resolved r = resolve_path(dirfd, rel, false);
    if (r.errno != WASI_OK) {
        return r.errno;
    }
    java.nio.file.Path p = java.nio.file.Paths.get(r.path);
    // A missing target is ENOENT before any shape check (Files.isDirectory is false for "missing" and "not a directory" alike).
    if (!java.nio.file.Files.exists(p, java.nio.file.LinkOption.NOFOLLOW_LINKS)) {
        return WASI_NOENT;
    }
    // Files.delete would happily remove a regular file or an empty directory;
    // rmdir must fail (ENOTDIR) on a non-directory, so pre-check.
    if (!java.nio.file.Files.isDirectory(p, java.nio.file.LinkOption.NOFOLLOW_LINKS)) {
        return WASI_NOTDIR;
    }
    // rmdir through a trailing slash on an existing directory is EINVAL per wasmtime.
    if (rel.endsWith("/")) {
        return WASI_INVAL;
    }
    try {
        java.nio.file.Files.delete(p);
    } catch (java.io.IOException ex) {
        return fs_errno(ex);
    }
    return WASI_OK;
}
