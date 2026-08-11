// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
// Create a symlink whose content is the guest's target string stored VERBATIM
// (never pre-resolved): confinement happens later, when something follows the
// link (resolve_path's realpath + containment check), not at creation.
// An absolute target is rejected outright — it could only ever escape the
// preopen tree at follow time. The link's own parent is resolved NOFOLLOW.
int wasi_path_symlink(int oldPtr, int oldLen, int fd, int newPtr, int newLen) {
    String target = new String(
        memory.read_string(Integer.toUnsignedLong(oldPtr), Integer.toUnsignedLong(oldLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    String newRel = new String(
        memory.read_string(Integer.toUnsignedLong(newPtr), Integer.toUnsignedLong(newLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    if (target.startsWith("/")) {
        return WASI_NOTCAPABLE;
    }
    Resolved r = resolve_path(fd, newRel, false);
    if (r.errno != WASI_OK) {
        return r.errno;
    }
    java.nio.file.Path linkPath = java.nio.file.Paths.get(r.path);
    // A trailing slash on the link path demands the name resolve to a directory,
    // so a symlink can never be created there: report why precisely (ENOENT if
    // nothing is there, EEXIST if a directory is, ENOTDIR if a plain file is).
    if (newRel.endsWith("/")) {
        if (!java.nio.file.Files.exists(linkPath, java.nio.file.LinkOption.NOFOLLOW_LINKS)) {
            return WASI_NOENT;
        }
        return java.nio.file.Files.isDirectory(linkPath, java.nio.file.LinkOption.NOFOLLOW_LINKS)
            ? WASI_EXIST
            : WASI_NOTDIR;
    }
    try {
        java.nio.file.Files.createSymbolicLink(linkPath, java.nio.file.Paths.get(target));
    } catch (java.io.IOException ex) {
        return fs_errno(ex);
    }
    return WASI_OK;
}
