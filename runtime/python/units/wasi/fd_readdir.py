# requires: memory/init, memory/i32_store, wasi/wasi_filetype
def wasi_fd_readdir(self, fd, buf_ptr, buf_len, cookie, bufused_ptr):
    entry = self.fds.get(fd)
    if not isinstance(entry, self.WasiDir):
        return self.ERRNO_BADF
    if not (self.fd_meta[fd][0] & self.RIGHTS_FD_READDIR):
        return self.ERRNO_NOTCAPABLE
    try:
        # cookie 0 starts a fresh enumeration, so re-scan the directory then;
        # a non-zero cookie resumes the snapshot cached from that start (the
        # opaque-resume-point contract).
        if entry.entries is None or cookie == 0:
            entry.entries = self.readdir_entries(entry.host_path)
        entries = entry.entries
        out = bytearray()
        i = cookie
        while i < len(entries) and len(out) < buf_len:
            name, filetype, ino = entries[i]
            # dirent: d_next (u64, resume cookie) + d_ino (u64) + d_namlen
            # (u32) + d_type (u8) + 3 pad, followed by the (unpadded) name.
            out += struct.pack("<QQIBxxx", i + 1, ino, len(name), filetype) + name
            i += 1
        # A dirent may be legally truncated at the tail once buf_len runs out.
        if len(out) > buf_len:
            out = out[:buf_len]
        self.memory.init(buf_ptr, bytes(out), 0, len(out))
        self.memory.i32_store(bufused_ptr, len(out))
    except OSError:
        return self.ERRNO_IO
    return self.ERRNO_SUCCESS

def readdir_entries(self, host_path):
    entries = [
        (b".", 3, os.lstat(host_path).st_ino),
        (b"..", 3, os.lstat(os.path.join(host_path, "..")).st_ino),
    ]
    for name in sorted(os.listdir(host_path)):
        st = os.lstat(os.path.join(host_path, name))
        entries.append((name.encode("utf-8", "surrogateescape"), self.wasi_filetype(st), st.st_ino))
    return entries
