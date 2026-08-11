// requires: wasi/errno_fs
// resolve_times turns fstflags + the two timestamps into the (atime, mtime)
// pair os.Chtimes expects, where a zero time.Time leaves that field unchanged.
// Setting both the explicit bit and the *_NOW bit for one field is EINVAL, the
// only validation the suite checks.
func (w *WASI) resolve_times(atim, mtim uint64, fstflags uint32) (time.Time, time.Time, uint32) {
    const (
        atimSet uint32 = 0x1
        atimNow uint32 = 0x2
        mtimSet uint32 = 0x4
        mtimNow uint32 = 0x8
    )
    if fstflags&atimSet != 0 && fstflags&atimNow != 0 {
        return time.Time{}, time.Time{}, wasiInval
    }
    if fstflags&mtimSet != 0 && fstflags&mtimNow != 0 {
        return time.Time{}, time.Time{}, wasiInval
    }
    now := time.Now()
    var atime, mtime time.Time
    if fstflags&atimSet != 0 {
        atime = time.Unix(0, int64(atim))
    } else if fstflags&atimNow != 0 {
        atime = now
    }
    if fstflags&mtimSet != 0 {
        mtime = time.Unix(0, int64(mtim))
    } else if fstflags&mtimNow != 0 {
        mtime = now
    }
    return atime, mtime, wasiOk
}
