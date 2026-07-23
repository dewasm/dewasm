# requires: mem/check, mem/i32_load, mem/i32_store
# Bytes are emitted with printf over an every-byte '\xHH' format, which is
# binary-safe (NUL, %, backslash); the format must stay single-quoted.
wasi_fd_write() {
  local __p=$1 __fd=$2 __iovs=$3 __iovs_len=$4 __nwritten_ptr=$5
  local -n __m=${__p}mem
  local -n __fds=${__p}wfds
  local -n __tell=${__p}wtell
  if [[ -z ${__fds[$__fd]-} ]]; then
    R0=8 # EBADF
    return 0
  fi
  if (( __fd != 1 && __fd != 2 )); then
    R0=8 # EBADF: only stdout/stderr are writable
    return 0
  fi
  local __out='' __chunk __bytes __i __j __ptr __len __total=0
  for (( __i = 0; __i < __iovs_len; __i++ )); do
    mem_i32_load "$__p" $(( __iovs + __i * 8 )) || return $?
    __ptr=$R0
    mem_i32_load "$__p" $(( __iovs + __i * 8 + 4 )) || return $?
    __len=$R0
    if (( __len == 0 )); then continue; fi
    mem_check "$__p" "$__ptr" "$__len" || return $?
    __bytes=()
    for (( __j = 0; __j < __len; __j++ )); do
      __bytes+=("$(( __m[__ptr + __j] ))")
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
