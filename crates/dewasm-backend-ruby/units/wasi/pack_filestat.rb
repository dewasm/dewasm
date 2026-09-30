# requires: wasi/wasi_filetype
# Packs a `File::Stat` into a WASI `filestat` (64 bytes).
# Its fields are `dev`, `ino`, `filetype` (+7 padding), `nlink`, `size`, `atim`/`mtim`/`ctim`.
# All are u64, with times in nanoseconds.
def pack_filestat(stat)
  [
    stat.dev, stat.ino, wasi_filetype(stat), stat.nlink, stat.size,
    stat.atime.tv_sec * 1_000_000_000 + stat.atime.tv_nsec,
    stat.mtime.tv_sec * 1_000_000_000 + stat.mtime.tv_nsec,
    stat.ctime.tv_sec * 1_000_000_000 + stat.ctime.tv_nsec
  ].pack("Q<Q<Cx7Q<Q<Q<Q<Q<")
end
private :pack_filestat
