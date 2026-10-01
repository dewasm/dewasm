// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
int wasi_path_rename(int oldDirfd, int oldPathPtr, int oldPathLen, int newDirfd, int newPathPtr,
                     int newPathLen) {
    String oldRel = new String(
        memory.read_string(Integer.toUnsignedLong(oldPathPtr), Integer.toUnsignedLong(oldPathLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    // `rename(2)` never follows trailing symbolic links.
    // It moves the link itself and replaces the destination link.
    Resolved oldR = resolve_path(oldDirfd, oldRel, false);
    if (oldR.errno != WASI_OK) {
        return oldR.errno;
    }
    String newRel = new String(
        memory.read_string(Integer.toUnsignedLong(newPathPtr), Integer.toUnsignedLong(newPathLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    Resolved newR = resolve_path(newDirfd, newRel, false);
    if (newR.errno != WASI_OK) {
        return newR.errno;
    }
    java.nio.file.Path oldP = java.nio.file.Paths.get(oldR.path);
    java.nio.file.Path newP = java.nio.file.Paths.get(newR.path);
    // Trailing slashes (issue #42): existing non-directories were ENOTDIR in `resolve_path`.
    // A nonexistent slash-suffixed destination is renamed without the slash, as Wasmtime strips it.
    // The normalized Path already has no slash.
    // `rename(2)` reports type mismatches between the endpoints with specific `errno` values.
    // Java's generic FileSystemException flattens those to EIO, so pre-check them:
    // - renaming a directory onto an existing non-directory is ENOTDIR;
    // - renaming a non-directory onto an existing directory is EISDIR.
    // The matching directory-onto-nonempty-directory case needs no pre-check.
    // It surfaces as DirectoryNotEmptyException -> ENOTEMPTY through `fs_errno`.
    if (java.nio.file.Files.exists(newP, java.nio.file.LinkOption.NOFOLLOW_LINKS)) {
        boolean oldIsDir = java.nio.file.Files.isDirectory(oldP, java.nio.file.LinkOption.NOFOLLOW_LINKS);
        boolean newIsDir = java.nio.file.Files.isDirectory(newP, java.nio.file.LinkOption.NOFOLLOW_LINKS);
        if (oldIsDir && !newIsDir) {
            return WASI_NOTDIR;
        }
        if (!oldIsDir && newIsDir) {
            return WASI_ISDIR;
        }
    }
    try {
        java.nio.file.Files.move(oldP, newP, java.nio.file.StandardCopyOption.REPLACE_EXISTING);
    } catch (java.io.IOException ex) {
        return fs_errno(ex);
    }
    return WASI_OK;
}
