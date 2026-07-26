// requires: memory/read_string, memory/i32_store, wasi/resolve_path, wasi/errno_fs
func (w *WASI) wasi_path_open(dirfd, dirflags, pathPtr, pathLen, oflags uint32, fsRightsBase, fsRightsInheriting uint64, fdflags, openedFdPtr uint32) uint32 {
    rel := string(w.memory.read_string(uint64(pathPtr), uint64(pathLen)))
    hostPath, err := w.resolve_path(dirfd, rel, true)
    if err != wasiOk {
        return err
    }
    if oflags&0x2 != 0 { // oflags::DIRECTORY
        // O_DIRECTORY distinguishes "missing" (ENOENT) from "not a directory"
        // (ENOTDIR); guests (wasi-libc's opendir) branch on the difference.
        info, e := os.Stat(hostPath)
        if e != nil {
            return wasiNoent
        }
        if !info.IsDir() {
            return wasiNotdir
        }
        w.fds[w.nextFd] = &wasiDir{hostPath: hostPath}
    } else {
        read := fsRightsBase&0x2 != 0   // rights::FD_READ
        write := fsRightsBase&0x40 != 0 // rights::FD_WRITE
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
        if fdflags&0x1 != 0 { // fdflags::APPEND
            flag |= os.O_APPEND
        }
        f, e := os.OpenFile(hostPath, flag, 0o644)
        if e != nil {
            return w.fs_errno(e)
        }
        w.fds[w.nextFd] = f
    }
    w.memory.i32_store(uint64(openedFdPtr), w.nextFd)
    w.nextFd++
    return wasiOk
}
