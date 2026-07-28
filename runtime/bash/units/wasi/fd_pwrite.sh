# requires: mem/check, mem/i32_load, mem/i32_store
# WASI fd_pwrite (ADR-34): byte-wise binary-safe write into a file fd's
# whole-file buffer at an explicit offset, leaving <p>wtell untouched and
# ignoring APPEND (pwrite is explicitly positioned per WASI/POSIX, unlike
# write); zero-fills any gap and marks the buffer dirty, same as fd_write's
# kind-2 path. stdio (kind 1) is ESPIPE; a directory fd is EBADF.
wasi_fd_pwrite() {
  local __p=$1 __fd=$2 __iovs=$3 __iovs_len=$4 __offset=$5 __nwritten_ptr=$6
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
  local -n __wwr=${__p}wwr
  if [[ ${__wwr[$__fd]} != 1 ]]; then
    R0=8 # EBADF: not opened for writing
    return 0
  fi
  local -n __wdirty=${__p}wdirty
  local -n __buf=${__p}wbuf${__fd}
  local LC_ALL=C
  local __i __j __ptr __len __total=0 __pos=$__offset __blen
  for (( __i = 0; __i < __iovs_len; __i++ )); do
    mem_i32_load "$__p" $(( __iovs + __i * 8 )) || return $?
    __ptr=$R0
    mem_i32_load "$__p" $(( __iovs + __i * 8 + 4 )) || return $?
    __len=$R0
    if (( __len == 0 )); then continue; fi
    mem_check "$__p" "$__ptr" "$__len" || return $?
    for (( __blen = ${#__buf[@]}; __blen < __pos; __blen++ )); do
      __buf[__blen]=0
    done
    for (( __j = 0; __j < __len; __j++ )); do
      __buf[__pos]=$(( __m[__ptr + __j] & 0xff ))
      (( __pos++, __total++ ))
    done
  done
  __wdirty[$__fd]=1
  mem_i32_store "$__p" "$__nwritten_ptr" "$__total" || return $?
  R0=0
  return 0
}
