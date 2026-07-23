# requires: mem/i64_store
# The offset is the byte counter fd_read/fd_write track per stdio fd.
wasi_fd_tell() {
  local __p=$1 __fd=$2 __out=$3
  local -n __fds=${__p}wfds
  local -n __tell=${__p}wtell
  if [[ -z ${__fds[$__fd]-} ]]; then
    R0=8 # EBADF
    return 0
  fi
  mem_i64_store "$__p" "$__out" "$(( __tell[__fd] ))" || return $?
  R0=0
  return 0
}
