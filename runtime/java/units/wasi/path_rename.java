// requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
int wasi_path_rename(int oldDirfd, int oldPathPtr, int oldPathLen, int newDirfd, int newPathPtr,
                     int newPathLen) {
    String oldRel = new String(
        memory.read_string(Integer.toUnsignedLong(oldPathPtr), Integer.toUnsignedLong(oldPathLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    // rename(2) never follows trailing symlinks: it moves the link itself and
    // replaces the destination link.
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
    try {
        java.nio.file.Files.move(
            java.nio.file.Paths.get(oldR.path), java.nio.file.Paths.get(newR.path),
            java.nio.file.StandardCopyOption.REPLACE_EXISTING);
    } catch (java.io.IOException ex) {
        return fs_errno(ex);
    }
    return WASI_OK;
}
