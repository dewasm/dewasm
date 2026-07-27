# WASI fd_filestat_set_size (ADR-32): truncates or zero-extends a file fd's
# whole-file buffer to <size> and marks it dirty (flushed on close/sync, D1).
# A negative size is EINVAL (mirrors truncate(2)/Ruby's IO#truncate); a
# directory or stdio fd is EBADF.
wasi_fd_filestat_set_size() {
  local __p=$1 __fd=$2 __size=$3
  local -n __fds=${__p}wfds
  if [[ ${__fds[$__fd]-} != 2 ]]; then
    R0=8 # EBADF
    return 0
  fi
  if (( __size < 0 )); then
    R0=28 # EINVAL
    return 0
  fi
  local -n __wbuf=${__p}wbuf${__fd}
  local -n __wdirty=${__p}wdirty
  local __cur=${#__wbuf[@]}
  local __i
  if (( __size < __cur )); then
    for (( __i = __size; __i < __cur; __i++ )); do
      unset "__wbuf[__i]"
    done
  elif (( __size > __cur )); then
    for (( __i = __cur; __i < __size; __i++ )); do
      __wbuf[__i]=0
    done
  fi
  __wdirty[$__fd]=1
  R0=0
  return 0
}
