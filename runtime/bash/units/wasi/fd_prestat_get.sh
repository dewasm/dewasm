# requires: mem/fill, mem/i32_store
# prestat for a preopen dir fd: tag u8 = 0 (dir) at +0, 3 pad, then the
# guest-name byte length as u32 at +4. Only a preopen (dir fd with <p>wname set)
# answers; anything else is EBADF (8), which is what stops a libc's preopen scan:
# preopens are dense from fd 3, so the first non-preopen fd ends the scan.
wasi_fd_prestat_get() {
  local __p=$1 __fd=$2 __out=$3
  local -n __fds=${__p}wfds
  local -n __wname=${__p}wname
  if [[ ${__fds[$__fd]-} != 3 || -z ${__wname[$__fd]-} ]]; then
    R0=8 # EBADF
    return 0
  fi
  local LC_ALL=C
  local __n=${#__wname[$__fd]}
  mem_fill "$__p" "$__out" 0 8 || return $?
  mem_i32_store "$__p" $(( __out + 4 )) "$__n" || return $?
  R0=0
  return 0
}
