// requires: memory/init
func (w *WASI) wasi_fd_prestat_get(fd, outPtr uint32) uint32 {
    d, ok := w.fds[fd].(*wasiDir)
    if !ok || d.preopenName == nil {
        return wasiBadf
    }
    // prestat: tag (u8, 0 = dir) + 3 pad + pr_name_len (u32).
    buf := make([]byte, 8)
    binary.LittleEndian.PutUint32(buf[4:8], uint32(len(d.preopenName)))
    w.memory.init(uint64(outPtr), buf, 0, 8)
    return wasiOk
}
