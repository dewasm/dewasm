// fd_advise is purely advisory: validate the advice code and return success.
// A directory fd (no *os.File) is BADF, matching the suite's accepted
// ISDIR/BADF/NOTCAPABLE for directory ops.
func (w *WASI) wasi_fd_advise(fd uint32, offset, length uint64, advice uint32) uint32 {
    if _, ok := w.fds[fd].(*os.File); !ok {
        return wasiBadf
    }
    if advice > 5 { // NORMAL, SEQUENTIAL, RANDOM, WILLNEED, DONTNEED, NOREUSE
        return wasiInval
    }
    return wasiOk
}
