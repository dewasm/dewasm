# requires: mem/i64_store
# A file fd (kind 2) seeks within its whole-file byte buffer (ADR-34): whence 0/1/2
# is SET/CUR/END against 0 / current offset / buffer length, a negative result is
# EINVAL (28), and the new offset is stored back. stdio pipes cannot seek (ESPIPE);
# a directory fd is EBADF. The i64 offset arrives as bash's signed-64 pattern.
wasi_fd_seek() {
  local __p=$1 __fd=$2 __offset=$3 __whence=$4 __out=$5
  local -n __fds=${__p}wfds
  local -n __tell=${__p}wtell
  local __kind=${__fds[$__fd]-}
  if [[ -z $__kind ]]; then
    R0=8 # EBADF
    return 0
  fi
  if [[ $__kind == 1 ]]; then
    R0=70 # ESPIPE: stdio pipes cannot seek
    return 0
  fi
  if [[ $__kind == 3 ]]; then
    R0=8 # EBADF: a directory has no seek offset
    return 0
  fi
  local -n __wrbase=${__p}wrbase
  if (( (__wrbase[$__fd] & 0x4) == 0 )); then
    R0=76 # ENOTCAPABLE: fd lacks FD_SEEK
    return 0
  fi
  local -n __buf=${__p}wbuf${__fd}
  local __base
  case $__whence in
    0) __base=0 ;;
    1) __base=${__tell[$__fd]} ;;
    2) __base=${#__buf[@]} ;;
    *) R0=28; return 0 ;; # EINVAL
  esac
  local __new=$(( __base + __offset ))
  if (( __new < 0 )); then
    R0=28 # EINVAL
    return 0
  fi
  __tell[$__fd]=$__new
  mem_i64_store "$__p" "$__out" "$__new" || return $?
  R0=0
  return 0
}
