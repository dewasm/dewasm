# requires: wasi/wasi_filetype
# Packs an os.stat_result into a WASI filestat (64 bytes): dev, ino, filetype
# (+7 pad), nlink, size, atim/mtim/ctim (all u64, times in nanoseconds).
def pack_filestat(self, st):
    return struct.pack(
        "<QQBxxxxxxxQQQQQ",
        st.st_dev,
        st.st_ino,
        self.wasi_filetype(st),
        st.st_nlink,
        st.st_size,
        st.st_atime_ns,
        st.st_mtime_ns,
        st.st_ctime_ns,
    )
