// requires: memory/read_string, memory/i32_store, wasi/resolve_path, wasi/errno_fs
int wasi_path_open(int dirfd, int dirflags, int pathPtr, int pathLen, int oflags, long fsRightsBase,
                   long fsRightsInheriting, int fdflags, int openedFdPtr) {
    String rel = new String(
        memory.read_string(Integer.toUnsignedLong(pathPtr), Integer.toUnsignedLong(pathLen)),
        java.nio.charset.StandardCharsets.UTF_8);
    // dirflags::SYMLINK_FOLLOW decides whether the final component is chased through a symlink.
    // Without it (the default) a symlink final component is
    // ELOOP, not silently opened as its target (the O_NOFOLLOW shape).
    boolean follow = (dirflags & 0x1) != 0;
    Resolved r = resolve_path(dirfd, rel, follow);
    if (r.errno != WASI_OK) {
        return r.errno;
    }
    // The opening dirfd must itself carry PATH_OPEN; O_TRUNC additionally spends the directory's PATH_FILESTAT_SET_SIZE right (truncation is a size change), both enforced with NOTCAPABLE.
    if (lacksRight(dirfd, R_PATH_OPEN)) {
        return WASI_NOTCAPABLE;
    }
    boolean trunc = (oflags & 0x8) != 0; // oflags::TRUNC
    if (trunc && lacksRight(dirfd, R_PATH_FILESTAT_SET_SIZE)) {
        return WASI_NOTCAPABLE;
    }
    java.nio.file.Path hostPath = java.nio.file.Paths.get(r.path);
    // A symlink final component reached without SYMLINK_FOLLOW is ELOOP.
    if (!follow && java.nio.file.Files.isSymbolicLink(hostPath)) {
        return WASI_LOOP;
    }
    boolean wantDir = (oflags & 0x2) != 0; // oflags::DIRECTORY
    boolean create = (oflags & 0x1) != 0; // oflags::CREAT
    boolean excl = (oflags & 0x4) != 0; // oflags::EXCL
    boolean trailingSlash = rel.endsWith("/");
    boolean exists = java.nio.file.Files.exists(hostPath);
    boolean isDir = java.nio.file.Files.isDirectory(hostPath);
    // The parent directory fd's inheriting rights cap what any child fd may hold; the opened fd's filetype then masks that down to the directory- relevant or file-relevant rights.
    FdMeta dm = meta.get(dirfd);
    long dirInh = (dm != null) ? dm.inheriting : (DIR_RIGHTS | FILE_RIGHTS);

    if (isDir) {
        // An existing directory, opened with or without O_DIRECTORY and with an optional trailing slash.
        // Requesting write access to a directory is
        // EISDIR (a directory has no byte stream to write).
        if ((fsRightsBase & R_FD_WRITE) != 0) {
            return WASI_ISDIR;
        }
        long base = fsRightsBase & dirInh & DIR_RIGHTS;
        long inh = fsRightsInheriting & dirInh & (DIR_RIGHTS | FILE_RIGHTS);
        fds.put(nextFd, new Dir(hostPath, null));
        meta.put(nextFd, new FdMeta(base, inh, fdflags));
    } else if (wantDir) {
        // O_DIRECTORY distinguishes "missing" (ENOENT) from "not a directory"
        // (ENOTDIR); guests (wasi-libc's opendir) branch on the difference.
        return exists ? WASI_NOTDIR : WASI_NOENT;
    } else if (trailingSlash) {
        // A slash-suffixed non-directory: ENOTDIR when existing, ENOENT when missing, except O_CREAT, which must not create through the slash;
        // per wasmtime: EINVAL on macOS, EISDIR on Linux.
        if (exists) {
            return WASI_NOTDIR;
        }
        if (create) {
            return System.getProperty("os.name").toLowerCase().contains("mac")
                ? WASI_INVAL
                : WASI_ISDIR;
        }
        return WASI_NOENT;
    } else {
        boolean read = (fsRightsBase & 0x2) != 0; // rights::FD_READ
        boolean write = (fsRightsBase & 0x40) != 0; // rights::FD_WRITE
        boolean append = (fdflags & 0x1) != 0; // fdflags::APPEND
        java.util.Set<java.nio.file.OpenOption> opts = new java.util.HashSet<>();
        if (read || (!read && !write)) {
            opts.add(java.nio.file.StandardOpenOption.READ);
        }
        // FileChannel.open silently ignores CREATE/CREATE_NEW/TRUNCATE_EXISTING unless the channel is opened for WRITE, so a create request with rights_base=0 would resolve to {READ, CREATE}, fail to create, and then error NoSuchFileException -> NOENT.
        // Force WRITE whenever a create/truncate option is present; the fd's reported and enforced rights still come from the rights model below.
        if (write || create || excl || trunc) {
            opts.add(java.nio.file.StandardOpenOption.WRITE);
        }
        if (create) {
            opts.add(java.nio.file.StandardOpenOption.CREATE);
        }
        if (excl) {
            opts.add(java.nio.file.StandardOpenOption.CREATE_NEW);
        }
        if (trunc) {
            opts.add(java.nio.file.StandardOpenOption.TRUNCATE_EXISTING);
        }
        java.nio.channels.FileChannel ch;
        try {
            ch = java.nio.channels.FileChannel.open(hostPath, opts);
        } catch (java.io.IOException ex) {
            return fs_errno(ex);
        }
        long base = fsRightsBase & dirInh & FILE_RIGHTS;
        long inh = fsRightsInheriting & dirInh & FILE_RIGHTS;
        fds.put(nextFd, new Handle(ch, hostPath, append));
        meta.put(nextFd, new FdMeta(base, inh, fdflags));
    }
    memory.i32_store(Integer.toUnsignedLong(openedFdPtr), nextFd);
    nextFd++;
    return WASI_OK;
}
