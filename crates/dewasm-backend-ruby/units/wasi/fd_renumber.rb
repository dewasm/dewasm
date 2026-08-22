def wasi_fd_renumber(from, to)
  src = @fds[from]
  return ERRNO_BADF if src.nil?
  dst = @fds[to]
  # Both endpoints must be open fds; renumbering onto an unused number is
  # ERRNO_BADF (matches wasmtime and the renumber testsuite trial).
  return ERRNO_BADF if dst.nil?
  # Close the descriptor being overwritten, unless it is stdio (whose real handle stays live) or a directory (no OS handle).
  dst.close if dst.respond_to?(:close) && !dst.is_a?(WasiDir) && !@std_ios.include?(dst)
  @fds[to] = src
  @fd_meta[to] = @fd_meta[from]
  @fds.delete(from)
  @fd_meta.delete(from)
  ERRNO_SUCCESS
end
