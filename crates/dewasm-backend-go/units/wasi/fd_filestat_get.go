// requires: memory/init, wasi/pack_filestat
func (w *WASI) wasi_fd_filestat_get(fd, bufPtr uint32) uint32 {
    entry, present := w.fds[fd]
    if !present {
        return wasiBadf
    }
    var fi os.FileInfo
    var err error
    if d, ok := entry.(*wasiDir); ok {
        fi, err = os.Stat(d.hostPath)
    } else if f, ok := entry.(*os.File); ok {
        fi, err = f.Stat()
    }
    if err != nil || fi == nil {
        return wasiIo
    }
    w.memory.init(uint64(bufPtr), w.pack_filestat(fi), 0, 64)
    return wasiOk
}
