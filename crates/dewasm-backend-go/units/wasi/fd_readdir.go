// requires: memory/init, memory/i32_store, wasi/wasi_filetype
func (w *WASI) wasi_fd_readdir(fd, bufPtr, bufLen uint32, cookie uint64, bufusedPtr uint32) uint32 {
    entry, ok := w.fds[fd].(*wasiDir)
    if !ok {
        return wasiBadf
    }
    if e := w.checkRight(fd, rightFdReaddir); e != wasiOk {
        return e
    }
    // A cookie of 0 starts a fresh enumeration, so re-snapshot the directory
    // (a file created since the previous listing must appear); continuation cookies read the cached snapshot for a stable resume.
    if !entry.loaded || cookie == 0 {
        entry.entries = w.readdir_entries(entry.hostPath)
        entry.loaded = true
    }
    out := []byte{}
    i := cookie
    for i < uint64(len(entry.entries)) && uint32(len(out)) < bufLen {
        e := entry.entries[i]
        // dirent: d_next (u64, resume cookie) + d_ino (u64) + d_namlen (u32) + d_type (u8) + 3 pad, followed by the (unpadded) name.
        hdr := make([]byte, 24)
        binary.LittleEndian.PutUint64(hdr[0:8], i+1)
        binary.LittleEndian.PutUint64(hdr[8:16], e.ino)
        binary.LittleEndian.PutUint32(hdr[16:20], uint32(len(e.name)))
        hdr[20] = e.filetype
        out = append(out, hdr...)
        out = append(out, e.name...)
        i++
    }
    // A dirent may be legally truncated at the tail once buf_len runs out.
    if uint32(len(out)) > bufLen {
        out = out[:bufLen]
    }
    w.memory.init(uint64(bufPtr), out, 0, uint64(len(out)))
    w.memory.i32_store(uint64(bufusedPtr), uint32(len(out)))
    return wasiOk
}

// The readdir cookie is a 1-based index into this snapshot, cached on the
// *wasiDir at the first call for that fd. os.ReadDir returns entries already sorted by name, matching the Python backend's explicit sort.
// Real inodes are read via Lstat + syscall.Stat_t.Ino (the portable field the filestat units already rely on) so d_ino matches fd_filestat_get.
func (w *WASI) readdir_entries(hostPath string) []wasiDirent {
    entries := []wasiDirent{
        {name: []byte("."), filetype: 3, ino: w.inode(hostPath)},
        {name: []byte(".."), filetype: 3, ino: w.inode(filepath.Join(hostPath, ".."))},
    }
    des, err := os.ReadDir(hostPath)
    if err != nil {
        return entries
    }
    for _, de := range des {
        ft := byte(0)
        if info, err := de.Info(); err == nil { // lstat: type of the link itself
            ft = w.wasi_filetype(info)
        }
        entries = append(entries, wasiDirent{
            name:     []byte(de.Name()),
            filetype: ft,
            ino:      w.inode(filepath.Join(hostPath, de.Name())),
        })
    }
    return entries
}

// inode returns the host inode of path (Lstat, so a symlink reports its own inode), or 0 when it cannot be determined.
func (w *WASI) inode(path string) uint64 {
    fi, err := os.Lstat(path)
    if err != nil {
        return 0
    }
    if st, ok := fi.Sys().(*syscall.Stat_t); ok {
        return uint64(st.Ino)
    }
    return 0
}
