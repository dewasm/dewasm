# requires: memory/init, memory/iws, wasi/wasi_filetype, wasi/rights
def wasi_fd_readdir(fd, buf_ptr, buf_len, cookie, bufused_ptr)
  entry = @fds[fd]
  return ERRNO_BADF unless entry.is_a?(WasiDir)
  return ERRNO_NOTCAPABLE unless fd_has_right?(fd, RIGHT_FD_READDIR)
  # cookie 0 starts a fresh scan, so re-snapshot the listing (a directory mutated between two full readdirs must show the new state); cookie > 0 continues the snapshot taken when this scan began.
  entry.entries = readdir_entries(entry.host_path) if cookie.zero? || entry.entries.nil?
  entries = entry.entries

  out = +"".b
  i = cookie
  while i < entries.size && out.bytesize < buf_len
    name, filetype, ino = entries[i]
    # dirent: d_next (u64, resume cookie) + d_ino (u64) + d_namlen (u32) + d_type (u8) + 3 pad, followed immediately by the (unpadded) name.
    out << [i + 1, ino, name.bytesize, filetype].pack("Q<Q<L<Cx3") << name.b
    i += 1
  end
  # A dirent may be legally truncated at the tail once buf_len runs out.
  out = out.byteslice(0, buf_len) if out.bytesize > buf_len
  @memory.init(buf_ptr, out, 0, out.bytesize)
  @memory.iws(bufused_ptr, out.bytesize)
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

def readdir_entries(host_path)
  entries = [
    [".", 3, File.stat(host_path).ino],
    ["..", 3, File.stat(File.join(host_path, "..")).ino],
  ]
  Dir.children(host_path).sort.each do |name|
    stat = File.lstat(File.join(host_path, name))
    entries << [name, wasi_filetype(stat), stat.ino]
  end
  entries
end
private :readdir_entries
