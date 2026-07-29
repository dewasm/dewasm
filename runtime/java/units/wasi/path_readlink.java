// requires: memory/read_string, memory/init, memory/i32_store, wasi/resolve_path, wasi/errno_fs
// Read a symlink's target into the guest buffer (ADR-40). The link itself is
// resolved NOFOLLOW; the target string is returned verbatim (as the guest wrote
// it at symlink time), truncated to buf_len — a short buffer takes the leading
// bytes, matching the WASI contract, with bufused reporting what was written.
int wasi_path_readlink(int fd, int pathPtr, int pathLen, int bufPtr, int bufLen, int bufusedPtr) {
    String rel = new String(
        memory.read_string(Integer.toUnsignedLong(pathPtr), Integer.toUnsignedLong(pathLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    Resolved r = resolve_path(fd, rel, false);
    if (r.errno != WASI_OK) {
        return r.errno;
    }
    java.nio.file.Path p = java.nio.file.Paths.get(r.path);
    // A trailing slash forces following, so an existing slash-suffixed name
    // resolves to a directory here (resolve_path already reported ENOTDIR for
    // a non-directory, issue #42) — and a directory is never a readable
    // symlink: EINVAL, as the host readlink(2) reports for "dir/" or
    // "symlink-to-dir/". A missing target still falls through to ENOENT.
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
