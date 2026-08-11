// requires: memory/read_string, memory/i32_store, wasi/resolve_path, wasi/errno_fs
func (w *WASI) wasi_path_open(dirfd, dirflags, pathPtr, pathLen, oflags uint32, fsRightsBase, fsRightsInheriting uint64, fdflags, openedFdPtr uint32) uint32 {
    rel := string(w.memory.read_string(uint64(pathPtr), uint64(pathLen)))
    symlinkFollow := dirflags&0x1 != 0 // lookupflags::SYMLINK_FOLLOW
    hostPath, err := w.resolve_path(dirfd, rel, symlinkFollow)
    if err != wasiOk {
        return err
    }
    // The base fd must carry PATH_OPEN; a rights-narrowed dir fd that
    // dropped it can no longer open beneath itself.
    if e := w.checkRight(dirfd, rightPathOpen); e != wasiOk {
        return e
    }
    // OFLAGS_TRUNC truncates on open; that spends the directory's
    // PATH_FILESTAT_SET_SIZE right, checked before the OS open can
    // touch the file.
    if oflags&0x8 != 0 { // oflags::TRUNC
        if e := w.checkRight(dirfd, rightPathFilestatSetSize); e != wasiOk {
            return e
        }
    }
    // Opening a directory read/write is ISDIR; the suite requires it for
    // O_DIRECTORY with FD_WRITE requested, before the directory is even stat'd.
    if oflags&0x2 != 0 && fsRightsBase&rightFdWrite != 0 {
        return wasiIsdir
    }
    // Without SYMLINK_FOLLOW, a final symlink component is ELOOP: resolve_path
    // left it unfollowed, so a plain Lstat detects it.
    if !symlinkFollow {
        if fi, e := os.Lstat(hostPath); e == nil && fi.Mode()&os.ModeSymlink != 0 {
            return wasiLoop
        }
    }

    // The rights a fd opened under this dir may hold are capped by the dir's
    // inheriting set.
    var inheritMask uint64 = 0xFFFFFFFFFFFFFFFF
    if m, ok := w.meta[dirfd]; ok {
        inheritMask = m.inheriting
    }
    grantedBase := fsRightsBase & inheritMask
    grantedInheriting := fsRightsInheriting & inheritMask

    // A trailing slash forces directory semantics; on a non-directory it is
    // NOTDIR (routed through the O_DIRECTORY path below), on a directory it
    // opens the directory.
    hasTrailingSlash := len(rel) > 0 && rel[len(rel)-1] == '/'

    if oflags&0x2 != 0 || hasTrailingSlash { // oflags::DIRECTORY
        // O_DIRECTORY distinguishes "missing" (ENOENT) from "not a directory"
        // (ENOTDIR); guests (wasi-libc's opendir) branch on the difference.
        info, e := os.Stat(hostPath)
        if e != nil {
            // O_CREAT must not create through a trailing slash; per wasmtime:
            // EINVAL on macOS, EISDIR on Linux, plain open ENOENT.
            if hasTrailingSlash && oflags&0x2 == 0 && oflags&0x1 != 0 {
                if runtime.GOOS == "darwin" {
                    return wasiInval
                }
                return wasiIsdir
            }
            return wasiNoent
        }
        if !info.IsDir() {
            return wasiNotdir
        }
        // A directory can never hold the per-file rights (FD_SEEK,
        // FD_FILESTAT_SET_SIZE, ...) even when requested: cap the grant to the
        // directory masks, so e.g. requesting FD_SEEK on a dir yields
        // a fd whose base lacks it.
        w.fds[w.nextFd] = &wasiDir{hostPath: hostPath}
        w.meta[w.nextFd] = &wasiFdMeta{
            base:       grantedBase & dirRightsBase,
            inheriting: grantedInheriting & dirRightsInheriting,
        }
    } else {
        read := fsRightsBase&rightFdRead != 0
        write := fsRightsBase&rightFdWrite != 0
        var flag int
        if read && write {
            flag = os.O_RDWR
        } else if write {
            flag = os.O_WRONLY
        } else {
            flag = os.O_RDONLY
        }
        if oflags&0x1 != 0 { // oflags::CREAT
            flag |= os.O_CREATE
        }
        if oflags&0x4 != 0 { // oflags::EXCL
            flag |= os.O_EXCL
        }
        if oflags&0x8 != 0 { // oflags::TRUNC
            flag |= os.O_TRUNC
        }
        // APPEND is NOT passed to the OS: it is tracked in fdflags so
        // fd_fdstat_set_flags can toggle it at runtime (fd_write seeks to end).
        f, e := os.OpenFile(hostPath, flag, 0o644)
        if e != nil {
            return w.fs_errno(e)
        }
        w.fds[w.nextFd] = f
        w.meta[w.nextFd] = &wasiFdMeta{base: grantedBase, inheriting: grantedInheriting, fdflags: uint16(fdflags)}
    }
    w.memory.i32_store(uint64(openedFdPtr), w.nextFd)
    w.nextFd++
    return wasiOk
}
