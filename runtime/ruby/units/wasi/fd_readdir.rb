# requires: memory/init, memory/i32_store, wasi/wasi_filetype
def wasi_fd_readdir(fd, buf_ptr, buf_len, cookie, bufused_ptr)
  entry = @fds[fd]
  return ERRNO_BADF unless entry.is_a?(WasiDir)
  entry.entries ||= readdir_entries(entry.host_path)
  entries = entry.entries

  out = +"".b
  i = cookie
  while i < entries.size && out.bytesize < buf_len
    name, filetype = entries[i]
    # dirent: d_next (u64, resume cookie) + d_ino (u64) + d_namlen (u32) +
    # d_type (u8) + 3 pad, followed immediately by the (unpadded) name.
    out << [i + 1, 0, name.bytesize, filetype].pack("Q<Q<L<Cx3") << name.b
    i += 1
  end
  # A dirent may be legally truncated at the tail once buf_len runs out.
  out = out.byteslice(0, buf_len) if out.bytesize > buf_len
  @memory.init(buf_ptr, out, 0, out.bytesize)
  @memory.i32_store(bufused_ptr, out.bytesize)
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end

def readdir_entries(host_path)
  entries = [[".", 3], ["..", 3]]
  Dir.children(host_path).sort.each do |name|
    entries << [name, wasi_filetype(File.lstat(File.join(host_path, name)))]
  end
  entries
end
private :readdir_entries
