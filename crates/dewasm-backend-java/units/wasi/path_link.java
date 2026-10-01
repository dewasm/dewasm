// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
// Create a hard link.
// Both endpoints are confined to their directory descriptors and resolved NOFOLLOW.
// So the link is made to the source name itself.
// A symbolic link source is hard-linked as the symbolic link, not its target.
// Following the source symbolic link (LOOKUPFLAGS_SYMLINK_FOLLOW) is rejected.
// Hard-linking a directory is EPERM; a trailing slash on the destination is ENOENT.
//
// macOS `link(2)` follows a symbolic link source.
// Linux does not, and neither does the AT_SYMLINK_NOFOLLOW `linkat` the suite expects.
// Files.createLink cannot express NOFOLLOW.
// So `createLink` on a symbolic link source follows the (dangling) link and throws.
// Emulate a NOFOLLOW hard link of a symbolic link by recreating it at the destination.
// This mirrors the Go backend's fallback.
int wasi_path_link(int oldFd, int oldFlags, int oldPtr, int oldLen, int newFd, int newPtr,
                   int newLen) {
    if ((oldFlags & 0x1) != 0) { // lookupflags::SYMLINK_FOLLOW
        return WASI_INVAL;
    }
    String oldRel = new String(
        memory.read_string(Integer.toUnsignedLong(oldPtr), Integer.toUnsignedLong(oldLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    String newRel = new String(
        memory.read_string(Integer.toUnsignedLong(newPtr), Integer.toUnsignedLong(newLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    Resolved oldR = resolve_path(oldFd, oldRel, false);
    if (oldR.errno != WASI_OK) {
        return oldR.errno;
    }
    Resolved newR = resolve_path(newFd, newRel, false);
    if (newR.errno != WASI_OK) {
        return newR.errno;
    }
    if (newRel.endsWith("/")) {
        return WASI_NOENT;
    }
    java.nio.file.Path oldP = java.nio.file.Paths.get(oldR.path);
    java.nio.file.Path newP = java.nio.file.Paths.get(newR.path);
    if (java.nio.file.Files.isDirectory(oldP, java.nio.file.LinkOption.NOFOLLOW_LINKS)) {
        return WASI_PERM;
    }
    try {
        java.nio.file.Files.createLink(newP, oldP);
    } catch (java.io.IOException ex) {
        if (java.nio.file.Files.isSymbolicLink(oldP)) {
            try {
                java.nio.file.Files.createSymbolicLink(newP, java.nio.file.Files.readSymbolicLink(oldP));
                return WASI_OK;
            } catch (java.io.IOException se) {
                return fs_errno(se);
            }
        }
        return fs_errno(ex);
    }
    return WASI_OK;
}
