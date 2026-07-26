// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
int wasi_path_create_directory(int dirfd, int pathPtr, int pathLen) {
    String rel = new String(
        memory.read_string(Integer.toUnsignedLong(pathPtr), Integer.toUnsignedLong(pathLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    // mkdir(2) never follows a trailing symlink (an existing one is EEXIST).
    Resolved r = resolve_path(dirfd, rel, false);
    if (r.errno != WASI_OK) {
        return r.errno;
    }
    try {
        java.nio.file.Files.createDirectory(java.nio.file.Paths.get(r.path));
    } catch (java.io.IOException ex) {
        return fs_errno(ex);
    }
    return WASI_OK;
}
