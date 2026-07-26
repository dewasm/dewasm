// requires: memory/init, wasi/errno_fs
func (w *WASI) wasi_fd_prestat_dir_name(fd, pathPtr, pathLen uint32) uint32 {
    d, ok := w.fds[fd].(*wasiDir)
    if !ok || d.preopenName == nil {
        return wasiBadf
    }
    name := d.preopenName
    if uint32(len(name)) > pathLen {
        return wasiNametoolong
    }
    w.memory.init(uint64(pathPtr), name, 0, uint64(len(name)))
    return wasiOk
}
