# requires: mem/fill, mem/i32_store8, mem/i64_store
# 24-byte fdstat: filetype u8 at +0 (2=char device if a tty, 4=regular
# file), flags/padding zeroed, rights_base/inheriting u64 = all bits.
wasi_fd_fdstat_get() {
  local __p=$1 __fd=$2 __out=$3
  local -n __fds=${__p}wfds
  if [[ -z ${__fds[$__fd]-} ]]; then
    R0=8 # EBADF
    return 0
  fi
  local __ft=4
  if [[ -t $__fd ]]; then __ft=2; fi
  mem_fill "$__p" "$__out" 0 24 || return $?
  mem_i32_store8 "$__p" "$__out" "$__ft" || return $?
  mem_i64_store "$__p" $(( __out + 8 )) -1 || return $?
  mem_i64_store "$__p" $(( __out + 16 )) -1 || return $?
  R0=0
  return 0
}
