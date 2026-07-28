# requires: wasi/rights
def wasi_fd_fdstat_set_rights(fd, base, inheriting)
  meta = @fd_meta[fd]
  return ERRNO_BADF if @fds[fd].nil? || meta.nil?
  # Rights can only be narrowed: requesting any bit the fd does not already
  # hold is ERRNO_NOTCAPABLE (ADR-40).
  return ERRNO_NOTCAPABLE if (base & ~meta[0]) != 0 || (inheriting & ~meta[1]) != 0
  meta[0] = base
  meta[1] = inheriting
  ERRNO_SUCCESS
end
