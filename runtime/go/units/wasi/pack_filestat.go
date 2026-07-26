// requires: wasi/wasi_filetype
// Pack an os.FileInfo into a WASI filestat (64 bytes): dev, ino, filetype
// (+7 pad), nlink, size, atim/mtim/ctim (all u64, times in nanoseconds). dev/
// ino/nlink come from the unix syscall.Stat_t when available; the times are
// taken from the portable ModTime (Go exposes no portable atim/ctim), which
// suffices for the guests we target (ADR-14).
func (w *WASI) pack_filestat(fi os.FileInfo) []byte {
    buf := make([]byte, 64)
    var dev, ino, nlink uint64
    nlink = 1
    if st, ok := fi.Sys().(*syscall.Stat_t); ok {
        dev = uint64(st.Dev)
        ino = uint64(st.Ino)
        nlink = uint64(st.Nlink)
    }
    mtime := uint64(fi.ModTime().UnixNano())
    binary.LittleEndian.PutUint64(buf[0:8], dev)
    binary.LittleEndian.PutUint64(buf[8:16], ino)
    buf[16] = w.wasi_filetype(fi)
    binary.LittleEndian.PutUint64(buf[24:32], nlink)
    binary.LittleEndian.PutUint64(buf[32:40], uint64(fi.Size()))
    binary.LittleEndian.PutUint64(buf[40:48], mtime)
    binary.LittleEndian.PutUint64(buf[48:56], mtime)
    binary.LittleEndian.PutUint64(buf[56:64], mtime)
    return buf
}
