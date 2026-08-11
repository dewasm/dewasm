# requires: mem/fill, mem/i32_store8, mem/i32_store16, mem/i64_store
# 24-byte fdstat: filetype u8 at +0 by fd kind (3 = directory, 4 = regular
# file, or for stdio 2 = char device when a tty else 4), fdflags u16 at
# +2, then the stored rights_base/inheriting u64 at +8/+16. The rights
# are what path_open granted (all-ones for stdio/preopens); reporting them
# instead of a flat all-ones is what the rights-narrowing conformance tests
# check.
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
  local -n __wrbase=${__p}wrbase
  local -n __wrinh=${__p}wrinh
  local -n __wfdflags=${__p}wfdflags
  mem_fill "$__p" "$__out" 0 24 || return $?
  mem_i32_store8 "$__p" "$__out" "$__ft" || return $?
  mem_i32_store16 "$__p" $(( __out + 2 )) "$(( __wfdflags[$__fd] ))" || return $?
  mem_i64_store "$__p" $(( __out + 8 )) "$(( __wrbase[$__fd] ))" || return $?
  mem_i64_store "$__p" $(( __out + 16 )) "$(( __wrinh[$__fd] ))" || return $?
  R0=0
  return 0
}
