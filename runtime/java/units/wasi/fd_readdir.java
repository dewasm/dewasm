// requires: memory/init, memory/i32_store, wasi/wasi_filetype
int wasi_fd_readdir(int fd, int bufPtr, int bufLen, long cookie, int bufusedPtr) {
    Object e = fds.get(fd);
    if (!(e instanceof Dir)) {
        return WASI_BADF;
    }
    Dir dir = (Dir) e;
    if (!dir.loaded) {
        dir.entries = readdir_entries(dir.hostPath);
        dir.loaded = true;
    }
    long lim = Integer.toUnsignedLong(bufLen);
    java.io.ByteArrayOutputStream out = new java.io.ByteArrayOutputStream();
    long i = cookie;
    while (i < dir.entries.size() && out.size() < lim) {
        Dirent ent = dir.entries.get((int) i);
        // dirent: d_next (u64, resume cookie) + d_ino (u64) + d_namlen (u32) +
        // d_type (u8) + 3 pad, followed by the (unpadded) name.
        byte[] hdr = new byte[24];
        java.nio.ByteBuffer h = java.nio.ByteBuffer.wrap(hdr).order(java.nio.ByteOrder.LITTLE_ENDIAN);
        h.putLong(0, i + 1);
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

// The readdir cookie is a 1-based index into this snapshot, cached on the Dir
// at the first call for that fd (ADR-14). "." and ".." lead; the rest are
// sorted by name so the listing is deterministic across runs.
private java.util.List<Dirent> readdir_entries(java.nio.file.Path hostPath) {
    java.util.List<Dirent> entries = new java.util.ArrayList<>();
    entries.add(new Dirent(".".getBytes(java.nio.charset.StandardCharsets.UTF_8), (byte) 3));
    entries.add(new Dirent("..".getBytes(java.nio.charset.StandardCharsets.UTF_8), (byte) 3));
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
                p.getFileName().toString().getBytes(java.nio.charset.StandardCharsets.UTF_8), ft));
        }
    } catch (java.io.IOException ex) {
        // An unreadable directory yields just "." / "..".
    }
    return entries;
}
