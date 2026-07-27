# requires: mem/check, mem/i32_load, mem/i32_store
# WASI fd_pread (ADR-34): byte-wise binary-safe read from a file fd's
# whole-file buffer at an explicit offset, leaving <p>wtell untouched
# (mirrors fd_read's kind-2 path exactly, minus the offset tracking). stdio
# (kind 1) cannot seek (ESPIPE, matching fd_seek/Ruby's ERRNO_SPIPE); a
# directory fd is EBADF (matching Ruby, which checks `io.is_a?(WasiDir)`
# before the stdio/ESPIPE check).
wasi_fd_pread() {
  local __p=$1 __fd=$2 __iovs=$3 __iovs_len=$4 __offset=$5 __nread_ptr=$6
  local -n __m=${__p}mem
  local -n __fds=${__p}wfds
  local __kind=${__fds[$__fd]-}
  if [[ -z $__kind ]]; then
    R0=8 # EBADF
    return 0
  fi
  if [[ $__kind != 2 ]]; then
    if [[ $__kind == 1 ]]; then
      R0=70 # ESPIPE
    else
      R0=8 # EBADF: directory
    fi
    return 0
  fi
  local -n __buf=${__p}wbuf${__fd}
  local __buflen=${#__buf[@]}
  local LC_ALL=C
  local __i __j __ptr __len __total=0 __pos=$__offset
  for (( __i = 0; __i < __iovs_len && __pos < __buflen; __i++ )); do
    mem_i32_load "$__p" $(( __iovs + __i * 8 )) || return $?
    __ptr=$R0
    mem_i32_load "$__p" $(( __iovs + __i * 8 + 4 )) || return $?
    __len=$R0
    if (( __len == 0 )); then continue; fi
    mem_check "$__p" "$__ptr" "$__len" || return $?
    for (( __j = 0; __j < __len && __pos < __buflen; __j++ )); do
      __m[__ptr + __j]=$(( __buf[__pos] & 0xff ))
      (( __pos++, __total++ ))
    done
  done
  mem_i32_store "$__p" "$__nread_ptr" "$__total" || return $?
  R0=0
  return 0
}
