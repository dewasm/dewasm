// requires: wasi/wasi_filetype
// Pack an `os.FileInfo` into a WASI `filestat` (64 bytes).
// The fields are `dev`, `ino`, `filetype` (+7 padding), `nlink`, `size`, `atim`/`mtim`/`ctim`.
// `atim`/`mtim`/`ctim` are each a u64, in nanoseconds.
// `dev`/`ino`/`nlink` come from the Unix `syscall.Stat_t`.
// `mtim` is the portable `ModTime`.
// `atim`/`ctim` are read from the `Stat_t`'s platform time field via `reflect`.
// That is the only way to reach it with the standard library and no build tags.
// That is because the field is named `Atim` on Linux and `Atimespec` on Darwin.
// A distinct `atim` matters for `fd_filestat_set_times`.
// That system call changes `mtim` while leaving `atim` untouched.
func (w *WASI) pack_filestat(fi os.FileInfo) []byte {
    buf := make([]byte, 64)
    var dev, ino, nlink uint64
    nlink = 1
    mtime := uint64(fi.ModTime().UnixNano())
    atime, ctime := mtime, mtime
    if st, ok := fi.Sys().(*syscall.Stat_t); ok {
        dev = uint64(st.Dev)
        ino = uint64(st.Ino)
        nlink = uint64(st.Nlink)
        if a := statTimeNanos(st, "Atim", "Atimespec"); a != 0 {
            atime = uint64(a)
        }
        if c := statTimeNanos(st, "Ctim", "Ctimespec"); c != 0 {
            ctime = uint64(c)
        }
    }
    binary.LittleEndian.PutUint64(buf[0:8], dev)
    binary.LittleEndian.PutUint64(buf[8:16], ino)
    buf[16] = w.wasi_filetype(fi)
    binary.LittleEndian.PutUint64(buf[24:32], nlink)
    binary.LittleEndian.PutUint64(buf[32:40], uint64(fi.Size()))
    binary.LittleEndian.PutUint64(buf[40:48], atime)
    binary.LittleEndian.PutUint64(buf[48:56], mtime)
    binary.LittleEndian.PutUint64(buf[56:64], ctime)
    return buf
}

// `statTimeNanos` reads a `syscall.Stat_t` `timespec` field as nanoseconds.
// The field is given by its per-platform names.
// It uses reflect, so a single build-tag-free file compiles on both platforms.
// Those are Linux (`Atim`/`Ctim`) and Darwin (`Atimespec`/`Ctimespec`).
// Returns 0 when no listed field is present.
func statTimeNanos(st *syscall.Stat_t, names ...string) int64 {
    v := reflect.ValueOf(st).Elem()
    for _, n := range names {
        f := v.FieldByName(n)
        if f.IsValid() {
            return f.FieldByName("Sec").Int()*1_000_000_000 + f.FieldByName("Nsec").Int()
        }
    }
    return 0
}
