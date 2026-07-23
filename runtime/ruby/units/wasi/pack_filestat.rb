# requires: wasi/wasi_filetype
# Packs a File::Stat into a WASI filestat (64 bytes): dev, ino, filetype
# (+7 pad), nlink, size, atim/mtim/ctim (all u64, times in nanoseconds).
def pack_filestat(stat)
  [
    stat.dev, stat.ino, wasi_filetype(stat), stat.nlink, stat.size,
    (stat.atime.to_r * 1_000_000_000).to_i,
    (stat.mtime.to_r * 1_000_000_000).to_i,
    (stat.ctime.to_r * 1_000_000_000).to_i
  ].pack("Q<Q<Cx7Q<Q<Q<Q<Q<")
end
private :pack_filestat
