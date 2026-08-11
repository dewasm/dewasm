# requires: wasi/wasi_filetype
# Packs an os.stat_result into a WASI filestat (64 bytes): dev, ino, filetype
# (+7 pad), nlink, size, atim/mtim/ctim (all u64, times in nanoseconds).
# The host fields are signed (dev_t is signed and macOS reports negative
# st_dev for pipes and devfs nodes; timestamps can sit before the epoch) and
# wasmtime copies the bits, so each 64-bit field is masked to u64
# rather than letting struct.pack range-check it (issue #132).
def pack_filestat(self, st):
    return struct.pack(
        "<QQBxxxxxxxQQQQQ",
        st.st_dev & 0xFFFFFFFFFFFFFFFF,
        st.st_ino & 0xFFFFFFFFFFFFFFFF,
        self.wasi_filetype(st),
        st.st_nlink & 0xFFFFFFFFFFFFFFFF,
        st.st_size & 0xFFFFFFFFFFFFFFFF,
        st.st_atime_ns & 0xFFFFFFFFFFFFFFFF,
        st.st_mtime_ns & 0xFFFFFFFFFFFFFFFF,
        st.st_ctime_ns & 0xFFFFFFFFFFFFFFFF,
    )
