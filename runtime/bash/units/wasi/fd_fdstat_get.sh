# requires: mem/fill, mem/i32_store8, mem/i64_store
# 24-byte fdstat: filetype u8 at +0 by fd kind — 3 = directory, 4 = regular
# file, or (stdio) 2 = char device when a tty else 4 (ADR-32) — flags/padding
# zeroed, rights_base/inheriting u64 = all bits.
wasi_fd_fdstat_get() {
  local __p=$1 __fd=$2 __out=$3
  local -n __fds=${__p}wfds
  local __kind=${__fds[$__fd]-}
  if [[ -z $__kind ]]; then
    R0=8 # EBADF
    return 0
  fi
  local __ft
  case $__kind in
    3) __ft=3 ;; # directory
    2) __ft=4 ;; # regular file
    *) __ft=4; [[ -t $__fd ]] && __ft=2 ;;
  esac
  mem_fill "$__p" "$__out" 0 24 || return $?
  mem_i32_store8 "$__p" "$__out" "$__ft" || return $?
  mem_i64_store "$__p" $(( __out + 8 )) -1 || return $?
  mem_i64_store "$__p" $(( __out + 16 )) -1 || return $?
  R0=0
  return 0
}
