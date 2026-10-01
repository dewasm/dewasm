// requires: wasi/wasi_filetype
// Pack a host file's attributes into a WASI `filestat` (64 bytes).
// The fields are `dev`, `ino`, `filetype` (+7 padding), `nlink`, `size`, `atim`/`mtim`/`ctim`.
// All are u64, and times are in nanoseconds.
// `dev`/`ino`/`nlink` come from the "unix:*" attribute view.
// That view needs platform support (macOS/Linux).
// The times all use the portable `lastModifiedTime` (Java exposes no accurate `atim`/`ctim`).
// That is enough for the guests we target.
// `follow` selects `stat` or `lstat`.
byte[] pack_filestat(java.nio.file.Path p, boolean follow) throws java.io.IOException {
    java.nio.file.LinkOption[] opts = follow
        ? new java.nio.file.LinkOption[0]
        : new java.nio.file.LinkOption[] {java.nio.file.LinkOption.NOFOLLOW_LINKS};
    java.nio.file.attribute.BasicFileAttributes a =
        java.nio.file.Files.readAttributes(p, java.nio.file.attribute.BasicFileAttributes.class, opts);
    long dev = 0;
    long ino = 0;
    long nlink = 1;
    try {
        java.util.Map<String, Object> u = java.nio.file.Files.readAttributes(p, "unix:dev,ino,nlink", opts);
        dev = ((Number) u.get("dev")).longValue();
        ino = ((Number) u.get("ino")).longValue();
        nlink = ((Number) u.get("nlink")).longValue();
    } catch (Exception e) {
        // A file system without Unix attributes: leave `dev`/`ino` at 0 and `nlink` at 1.
    }
    // Report the three timestamps (`atim`/`mtim`/`ctim`) separately.
    // Do not collapse them to `mtime`.
    // A guest sets one and checks the others stay put (`fd_filestat_set_times`).
    // That guest then sees the distinction.
    // Java exposes no accurate `ctim`.
    // So the change-time slot reuses `creationTime` as a best-effort stand-in.
    long atime = a.lastAccessTime().to(java.util.concurrent.TimeUnit.NANOSECONDS);
    long mtime = a.lastModifiedTime().to(java.util.concurrent.TimeUnit.NANOSECONDS);
    long ctime = a.creationTime().to(java.util.concurrent.TimeUnit.NANOSECONDS);
    byte[] buf = new byte[64];
    java.nio.ByteBuffer bb = java.nio.ByteBuffer.wrap(buf).order(java.nio.ByteOrder.LITTLE_ENDIAN);
    bb.putLong(0, dev);
    bb.putLong(8, ino);
    buf[16] = wasi_filetype(a);
    bb.putLong(24, nlink);
    bb.putLong(32, a.size());
    bb.putLong(40, atime);
    bb.putLong(48, mtime);
    bb.putLong(56, ctime);
    return buf;
}
