# requires: mem/i64_store
# For a file fd (kind 2) the offset is its buffer position; for stdio it is the
# byte counter fd_read/fd_write track (ADR-12). A directory fd is EBADF.
wasi_fd_tell() {
  local __p=$1 __fd=$2 __out=$3
  local -n __fds=${__p}wfds
  local -n __tell=${__p}wtell
  local __kind=${__fds[$__fd]-}
  if [[ -z $__kind ]]; then
    R0=8 # EBADF
    return 0
  fi
  if [[ $__kind == 3 ]]; then
    R0=8 # EBADF: a directory has no offset
    return 0
  fi
  mem_i64_store "$__p" "$__out" "$(( __tell[$__fd] ))" || return $?
  R0=0
  return 0
}
