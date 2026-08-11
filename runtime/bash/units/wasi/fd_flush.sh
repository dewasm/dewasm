# requires: wasi/file_flush
# wasi_fd_flush <p> <fd>: flush a file fd's buffer to disk if it is dirty, then clear the dirty flag.
# A no-op for non-file fds or a clean buffer.
# R0 is the errno (from file_flush; 0 for the no-op paths).
wasi_fd_flush() {
  local __p=$1 __fd=$2
  local -n __fds=${__p}wfds
  local -n __dirty=${__p}wdirty
  local -n __wpath=${__p}wpath
  if [[ ${__fds[$__fd]-} != 2 || ${__dirty[$__fd]-} != 1 ]]; then
    R0=0
    return 0
  fi
  wasi_file_flush "${__p}wbuf${__fd}" "${__wpath[$__fd]}"
  __dirty[$__fd]=0
  return 0
}
