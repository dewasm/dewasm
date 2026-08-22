// requires: memory/read_string, memory/init, wasi/resolve_path, wasi/pack_filestat, wasi/errno_fs
func (w *WASI) wasi_path_filestat_get(dirfd, flags, pathPtr, pathLen, bufPtr uint32) uint32 {
    rel := string(w.memory.read_string(uint64(pathPtr), uint64(pathLen)))
    symlinkFollow := flags&0x1 != 0 // lookupflags::SYMLINK_FOLLOW
    hostPath, err := w.resolve_path(dirfd, rel, symlinkFollow)
    if err != wasiOk {
        return err
    }
    var fi os.FileInfo
    var e error
    if symlinkFollow {
        fi, e = os.Stat(hostPath)
    } else {
        fi, e = os.Lstat(hostPath)
    }
    if e != nil {
        return w.fs_errno(e)
    }
    w.memory.init(uint64(bufPtr), w.pack_filestat(fi), 0, 64)
    return wasiOk
}
