// requires: memory/read_string, memory/init, memory/i32_store, wasi/resolve_path, wasi/errno_fs
// The link itself is resolved NOFOLLOW.
// The target string is returned unchanged, as the guest wrote it when creating the symbolic link.
// It is truncated to `buf_len`: a short buffer takes the leading bytes, matching the WASI contract.
// `bufused` reports what was written.
int wasi_path_readlink(int fd, int pathPtr, int pathLen, int bufPtr, int bufLen, int bufusedPtr) {
    String rel = new String(
        memory.read_string(Integer.toUnsignedLong(pathPtr), Integer.toUnsignedLong(pathLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    Resolved r = resolve_path(fd, rel, false);
    if (r.errno != WASI_OK) {
        return r.errno;
    }
    java.nio.file.Path p = java.nio.file.Paths.get(r.path);
    // An existing slash-suffixed name resolved (following) to a directory.
    // Non-directories were ENOTDIR in `resolve_path`.
    // A directory is not a symbolic link: EINVAL, like the host `readlink(2)`.
    // Missing falls through.
    if (rel.endsWith("/") && java.nio.file.Files.exists(p)) {
        return WASI_INVAL;
    }
    java.nio.file.Path target;
    try {
        target = java.nio.file.Files.readSymbolicLink(p);
    } catch (java.io.IOException ex) {
        return fs_errno(ex);
    }
    byte[] bytes = target.toString().getBytes(java.nio.charset.StandardCharsets.UTF_8);
    int n = Math.min(bytes.length, bufLen);
    memory.init(Integer.toUnsignedLong(bufPtr), bytes, 0, n);
    memory.i32_store(Integer.toUnsignedLong(bufusedPtr), n);
    return WASI_OK;
}
