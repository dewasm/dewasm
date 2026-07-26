// requires: memory/read_string, memory/i32_store, wasi/resolve_path, wasi/errno_fs
int wasi_path_open(int dirfd, int dirflags, int pathPtr, int pathLen, int oflags, long fsRightsBase,
                   long fsRightsInheriting, int fdflags, int openedFdPtr) {
    String rel = new String(
        memory.read_string(Integer.toUnsignedLong(pathPtr), Integer.toUnsignedLong(pathLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    Resolved r = resolve_path(dirfd, rel, true);
    if (r.errno != WASI_OK) {
        return r.errno;
    }
    java.nio.file.Path hostPath = java.nio.file.Paths.get(r.path);
    if ((oflags & 0x2) != 0) { // oflags::DIRECTORY
        // O_DIRECTORY distinguishes "missing" (ENOENT) from "not a directory"
        // (ENOTDIR); guests (wasi-libc's opendir) branch on the difference.
        if (!java.nio.file.Files.exists(hostPath)) {
            return WASI_NOENT;
        }
        if (!java.nio.file.Files.isDirectory(hostPath)) {
            return WASI_NOTDIR;
        }
        fds.put(nextFd, new Dir(hostPath, null));
    } else {
        boolean read = (fsRightsBase & 0x2) != 0; // rights::FD_READ
        boolean write = (fsRightsBase & 0x40) != 0; // rights::FD_WRITE
        boolean append = (fdflags & 0x1) != 0; // fdflags::APPEND
        java.util.Set<java.nio.file.OpenOption> opts = new java.util.HashSet<>();
        if (read || (!read && !write)) {
            opts.add(java.nio.file.StandardOpenOption.READ);
        }
        if (write) {
            opts.add(java.nio.file.StandardOpenOption.WRITE);
        }
        if ((oflags & 0x1) != 0) { // oflags::CREAT
            opts.add(java.nio.file.StandardOpenOption.CREATE);
        }
        if ((oflags & 0x4) != 0) { // oflags::EXCL
            opts.add(java.nio.file.StandardOpenOption.CREATE_NEW);
        }
        if ((oflags & 0x8) != 0) { // oflags::TRUNC
            opts.add(java.nio.file.StandardOpenOption.TRUNCATE_EXISTING);
        }
        java.nio.channels.FileChannel ch;
        try {
            ch = java.nio.channels.FileChannel.open(hostPath, opts);
        } catch (java.io.IOException ex) {
            return fs_errno(ex);
        }
        fds.put(nextFd, new Handle(ch, hostPath, append));
    }
    memory.i32_store(Integer.toUnsignedLong(openedFdPtr), nextFd);
    nextFd++;
    return WASI_OK;
}
