func (w *WASI) wasi_fd_filestat_set_size(fd uint32, size uint64) uint32 {
    f, ok := w.fds[fd].(*os.File)
    if !ok {
        return wasiBadf
    }
    if e := w.checkRight(fd, rightFdFilestatSetSize); e != wasiOk {
        return e
    }
    if err := f.Truncate(int64(size)); err != nil {
        return wasiIo
    }
    return wasiOk
}
