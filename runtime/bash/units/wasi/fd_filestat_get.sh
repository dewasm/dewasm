# requires: wasi/pack_filestat
# WASI fd_filestat_get: a directory fd (kind 3) reports type 3, size
# 0 (directory sizes aren't tracked); a file fd (kind 2) reports type 4 and
# size = its live whole-file buffer length (`${#buf[@]}`), which is coherent
# with unflushed writes. Unlike stat(2)ing the on-disk file, this sees bytes
# the guest wrote but hasn't synced yet. Stdio reuses fd_fdstat_get's own
# tty-vs-regular convention (type 2 if a tty, else 4) so the two calls agree
# on what a stdio fd "is".
wasi_fd_filestat_get() {
  local __p=$1 __fd=$2 __buf=$3
  local -n __fds=${__p}wfds
  local __kind=${__fds[$__fd]-}
  if [[ -z $__kind ]]; then
    R0=8 # EBADF
    return 0
  fi
  local __ft __size=0
  if [[ $__kind == 3 ]]; then
    __ft=3 # directory
  elif [[ $__kind == 2 ]]; then
    __ft=4 # regular_file
    local -n __wbuf=${__p}wbuf${__fd}
    __size=${#__wbuf[@]}
  else
    __ft=4
    [[ -t $__fd ]] && __ft=2 # character_device (tty); else a regular file
  fi
  wasi_pack_filestat "$__p" "$__buf" "$__ft" "$__size" || return $?
  R0=0
  return 0
}
