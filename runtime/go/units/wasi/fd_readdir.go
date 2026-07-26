// requires: memory/init, memory/i32_store, wasi/wasi_filetype
func (w *WASI) wasi_fd_readdir(fd, bufPtr, bufLen uint32, cookie uint64, bufusedPtr uint32) uint32 {
    entry, ok := w.fds[fd].(*wasiDir)
    if !ok {
        return wasiBadf
    }
    if !entry.loaded {
        entry.entries = w.readdir_entries(entry.hostPath)
        entry.loaded = true
    }
    out := []byte{}
    i := cookie
    for i < uint64(len(entry.entries)) && uint32(len(out)) < bufLen {
        e := entry.entries[i]
        // dirent: d_next (u64, resume cookie) + d_ino (u64) + d_namlen (u32) +
        // d_type (u8) + 3 pad, followed by the (unpadded) name.
        hdr := make([]byte, 24)
        binary.LittleEndian.PutUint64(hdr[0:8], i+1)
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
// *wasiDir at the first call for that fd (ADR-14). os.ReadDir returns entries
// already sorted by name, matching the Python backend's explicit sort.
func (w *WASI) readdir_entries(hostPath string) []wasiDirent {
    entries := []wasiDirent{{name: []byte("."), filetype: 3}, {name: []byte(".."), filetype: 3}}
    des, err := os.ReadDir(hostPath)
    if err != nil {
        return entries
    }
    for _, de := range des {
        ft := byte(0)
        if info, err := de.Info(); err == nil { // lstat: type of the link itself
            ft = w.wasi_filetype(info)
        }
        entries = append(entries, wasiDirent{name: []byte(de.Name()), filetype: ft})
    }
    return entries
}
