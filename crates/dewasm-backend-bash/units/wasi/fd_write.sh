# requires: mem/check, mem/i32_load, mem/i32_store
# A file fd (kind 2) writes into its whole-file byte buffer at the current offset
# (or the buffer end when opened with APPEND), zero-filling any gap and marking the buffer dirty; it is flushed to disk on close/sync. stdout/stderr
# (kind 1) emit bytes through an every-byte '\xHH' printf format, which is binary-safe (NUL, %, backslash); the format must stay single-quoted.
wasi_fd_write() {
  local __p=$1 __fd=$2 __iovs=$3 __iovs_len=$4 __nwritten_ptr=$5
  local -n __m=${__p}mem
  local -n __fds=${__p}wfds
  local -n __tell=${__p}wtell
  local __kind=${__fds[$__fd]-}
  if [[ -z $__kind ]]; then
    R0=8 # EBADF
    return 0
  fi
  if [[ $__kind == 3 ]]; then
    R0=8 # EBADF: a directory is not writable
    return 0
  fi
  local LC_ALL=C
  local __i __j __ptr __len __total=0 __mk
  if [[ $__kind == 2 ]]; then
    local -n __wrbase=${__p}wrbase
    local -n __wapp=${__p}wapp
    local -n __wdirty=${__p}wdirty
    if (( (__wrbase[$__fd] & 0x40) == 0 )); then
      R0=76 # ENOTCAPABLE: fd lacks FD_WRITE
      return 0
    fi
    local -n __buf=${__p}wbuf${__fd}
    local __pos=${__tell[$__fd]} __blen
    if [[ ${__wapp[$__fd]} == 1 ]]; then __pos=${#__buf[@]}; fi
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
        __mk=$(( __ptr + __j ))
        __buf[__pos]=$(( __m[$__mk] & 0xff ))
        (( __pos++, __total++ ))
      done
    done
    __tell[$__fd]=$__pos
    __wdirty[$__fd]=1
    mem_i32_store "$__p" "$__nwritten_ptr" "$__total" || return $?
    R0=0
    return 0
  fi
  if (( __fd != 1 && __fd != 2 )); then
    R0=8 # EBADF: only stdout/stderr are writable
    return 0
  fi
  local __out='' __chunk __bytes
  for (( __i = 0; __i < __iovs_len; __i++ )); do
    mem_i32_load "$__p" $(( __iovs + __i * 8 )) || return $?
    __ptr=$R0
    mem_i32_load "$__p" $(( __iovs + __i * 8 + 4 )) || return $?
    __len=$R0
    if (( __len == 0 )); then continue; fi
    mem_check "$__p" "$__ptr" "$__len" || return $?
    __bytes=()
    for (( __j = 0; __j < __len; __j++ )); do
      __mk=$(( __ptr + __j ))
      __bytes+=("$(( __m[$__mk] ))")
    done
    printf -v __chunk '\\x%02x' "${__bytes[@]}"
    __out+=$__chunk
    (( __total += __len ))
  done
  if (( __fd == 1 )); then
    printf "$__out"
  else
    printf "$__out" >&2
  fi
  (( __tell[__fd] += __total ))
  mem_i32_store "$__p" "$__nwritten_ptr" "$__total" || return $?
  R0=0
  return 0
}
