// requires: memory/read_string, memory/init, wasi/resolve_path, wasi/pack_filestat, wasi/errno_fs
int wasi_path_filestat_get(int dirfd, int flags, int pathPtr, int pathLen, int bufPtr) {
    String rel = new String(
        memory.read_string(Integer.toUnsignedLong(pathPtr), Integer.toUnsignedLong(pathLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    boolean symlinkFollow = (flags & 0x1) != 0; // lookupflags::SYMLINK_FOLLOW
    Resolved r = resolve_path(dirfd, rel, symlinkFollow);
    if (r.errno != WASI_OK) {
        return r.errno;
    }
    try {
        byte[] stat = pack_filestat(java.nio.file.Paths.get(r.path), symlinkFollow);
        memory.init(Integer.toUnsignedLong(bufPtr), stat, 0, 64);
    } catch (java.io.IOException ex) {
        return fs_errno(ex);
    }
    return WASI_OK;
}
