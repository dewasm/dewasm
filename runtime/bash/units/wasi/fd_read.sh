# requires: mem/check, mem/i32_load, mem/i32_store
# Byte-wise binary-safe stdin: `read -d '' -n 1` yields '' for a NUL byte
# but fails on EOF, distinguishing the two; LC_ALL=C keeps reads and the
# ordinal conversion byte-granular.
wasi_fd_read() {
  local __p=$1 __fd=$2 __iovs=$3 __iovs_len=$4 __nread_ptr=$5
  local -n __m=${__p}mem
  local -n __fds=${__p}wfds
  local -n __tell=${__p}wtell
  if [[ -z ${__fds[$__fd]-} ]]; then
    R0=8 # EBADF
    return 0
  fi
  if (( __fd != 0 )); then
    R0=8 # EBADF: only stdin is readable
    return 0
  fi
  local LC_ALL=C
  local __i __j __ptr __len __total=0 __ch __b __eof=0
  for (( __i = 0; __i < __iovs_len && __eof == 0; __i++ )); do
    mem_i32_load "$__p" $(( __iovs + __i * 8 )) || return $?
    __ptr=$R0
    mem_i32_load "$__p" $(( __iovs + __i * 8 + 4 )) || return $?
    __len=$R0
    if (( __len == 0 )); then continue; fi
    mem_check "$__p" "$__ptr" "$__len" || return $?
    for (( __j = 0; __j < __len; __j++ )); do
      if ! IFS= read -r -d '' -n 1 __ch; then
        __eof=1
        break
      fi
      if [[ -z $__ch ]]; then
        __b=0
      else
        printf -v __b '%d' "'$__ch"
      fi
      __m[__ptr + __j]=$(( __b & 0xff ))
      (( __total += 1 ))
    done
  done
  (( __tell[0] += __total ))
  mem_i32_store "$__p" "$__nread_ptr" "$__total" || return $?
  R0=0
  return 0
}
