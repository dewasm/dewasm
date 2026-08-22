// requires: memory/init, memory/i32_store, wasi/wasi_filetype
int wasi_fd_readdir(int fd, int bufPtr, int bufLen, long cookie, int bufusedPtr) {
    Object e = fds.get(fd);
    if (!(e instanceof Dir)) {
        return WASI_BADF;
    }
    if (lacksRight(fd, R_FD_READDIR)) {
        return WASI_NOTCAPABLE;
    }
    Dir dir = (Dir) e;
    // Re-snapshot on every cookie-0 (fresh-start) call so a directory mutated between full reads is seen; a nonzero cookie resumes the snapshot the cookie was minted against (the opaque-resume-point contract).
    if (!dir.loaded || cookie == 0) {
        dir.entries = readdir_entries(dir.hostPath);
        dir.loaded = true;
    }
    long lim = Integer.toUnsignedLong(bufLen);
    java.io.ByteArrayOutputStream out = new java.io.ByteArrayOutputStream();
    // u64 cookie in a signed long: compare unsigned so a high-bit cookie is past-the-end (empty result), not a negative subscript.
    long i = cookie;
    while (Long.compareUnsigned(i, dir.entries.size()) < 0 && out.size() < lim) {
        Dirent ent = dir.entries.get((int) i);
        // dirent: d_next (u64, resume cookie) + d_ino (u64) + d_namlen (u32) + d_type (u8) + 3 pad, followed by the (unpadded) name.
        byte[] hdr = new byte[24];
        java.nio.ByteBuffer h = java.nio.ByteBuffer.wrap(hdr).order(java.nio.ByteOrder.LITTLE_ENDIAN);
        h.putLong(0, i + 1);
        h.putLong(8, ent.ino);
        h.putInt(16, ent.name.length);
        hdr[20] = ent.filetype;
        out.write(hdr, 0, 24);
        out.write(ent.name, 0, ent.name.length);
        i++;
    }
    byte[] bytes = out.toByteArray();
    // A dirent may be legally truncated at the tail once buf_len runs out.
    if (bytes.length > lim) {
        bytes = java.util.Arrays.copyOf(bytes, (int) lim);
    }
    memory.init(Integer.toUnsignedLong(bufPtr), bytes, 0, bytes.length);
    memory.i32_store(Integer.toUnsignedLong(bufusedPtr), bytes.length);
    return WASI_OK;
}

// The readdir cookie is a 1-based index into this snapshot, cached on the Dir at the first call for that fd. "." and ".." lead; the rest are sorted by name so the listing is deterministic across runs.
private java.util.List<Dirent> readdir_entries(java.nio.file.Path hostPath) {
    java.util.List<Dirent> entries = new java.util.ArrayList<>();
    // "." and ".." carry the real inode of the directory and its parent so a guest that cross-checks d_ino against fd_filestat_get(dir).ino agrees
    // (the parent's ino falls back to 0 outside the sandbox).
    entries.add(new Dirent(
        ".".getBytes(java.nio.charset.StandardCharsets.UTF_8), (byte) 3, readdir_ino(hostPath)));
    entries.add(new Dirent(
        "..".getBytes(java.nio.charset.StandardCharsets.UTF_8), (byte) 3,
        readdir_ino(hostPath.getParent())));
    try (java.nio.file.DirectoryStream<java.nio.file.Path> ds = java.nio.file.Files.newDirectoryStream(hostPath)) {
        java.util.List<java.nio.file.Path> kids = new java.util.ArrayList<>();
        for (java.nio.file.Path p : ds) {
            kids.add(p);
        }
        kids.sort((a, b) -> a.getFileName().toString().compareTo(b.getFileName().toString()));
        for (java.nio.file.Path p : kids) {
            byte ft = 0;
            try {
                java.nio.file.attribute.BasicFileAttributes a = java.nio.file.Files.readAttributes(
                    p, java.nio.file.attribute.BasicFileAttributes.class,
                    java.nio.file.LinkOption.NOFOLLOW_LINKS); // lstat: type of the link itself
                ft = wasi_filetype(a);
            } catch (java.io.IOException ex) {
                // Leave the type as unknown (0).
            }
            entries.add(new Dirent(
                p.getFileName().toString().getBytes(java.nio.charset.StandardCharsets.UTF_8), ft,
                readdir_ino(p)));
        }
    } catch (java.io.IOException ex) {
        // An unreadable directory yields just "." / "..".
    }
    return entries;
}

// The host inode of a path via the "unix:ino" attribute view (the same source pack_filestat reads), or 0 on a non-unix filesystem or a null/unreadable path.
// NOFOLLOW so a symlink entry reports the link's own inode.
private static long readdir_ino(java.nio.file.Path p) {
    if (p == null) {
        return 0;
    }
    try {
        return ((Number) java.nio.file.Files.getAttribute(
            p, "unix:ino", java.nio.file.LinkOption.NOFOLLOW_LINKS)).longValue();
    } catch (Exception ex) {
        return 0;
    }
}
