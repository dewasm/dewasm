# requires: memory/iwl, memory/iws, memory/init, wasi/rights
def wasi_fd_read(fd, iovs_ptr, iovs_len, nread_ptr)
  io = @fds[fd]
  return ERRNO_BADF if io.nil? || io.is_a?(WasiDir)
  return ERRNO_NOTCAPABLE unless fd_has_right?(fd, RIGHT_FD_READ)
  stdin = @std_ios[0].equal?(io)
  nread = 0
  iovs_len.times do |i|
    ptr = @memory.iwl(iovs_ptr + i * 8)
    len = @memory.iwl(iovs_ptr + i * 8 + 4)
    next if len == 0
    # stdin may be an interactive tty (the QuickJS REPL under a pty): a buffered
    # IO#read(len) blocks until the full len arrives or EOF, deadlocking a line-buffered terminal that never sends EOF. readpartial returns as soon as any bytes are available (the WASI short-read semantics wasmtime uses), while poll_oneoff's IO.select already did the waiting.
    # Files keep #read.
    if stdin
      begin
        chunk = io.readpartial(len)
      rescue EOFError
        break
      end
    else
      chunk = io.read(len)
      break if chunk.nil?
    end
    @memory.init(ptr, chunk, 0, chunk.bytesize)
    nread += chunk.bytesize
    break if chunk.bytesize < len
  end
  @memory.iws(nread_ptr, nread)
  ERRNO_SUCCESS
rescue SystemCallError
  ERRNO_IO
end
