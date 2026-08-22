# requires: mem/check, mem/i32_load, mem/i32_store
# Byte-wise binary-safe reads.
# A file fd (kind 2) copies from its whole-file byte buffer at the current offset; stdin (kind 1, fd 0) consumes the pushback buffer first (<p>wpush, a space-separated byte-ordinal list shared with poll_oneoff), then reads live.
# A non-tty stdin reads via `read -d '' -n
# 1`, where '' with success is a NUL byte and failure is EOF.
# A tty stdin instead reads a whole canonical line into the pushback buffer with a plain
# `read`: bash's `-n`/`-N`/`-t` reads toggle ICANON per call and each restore makes the pty line discipline re-echo the still-pending line, so the tty path must not use them.
# Bash strings cannot hold NUL, but canonical tty line input never contains one.
# A directory fd is EISDIR.
# LC_ALL=C keeps reads and ordinal conversion byte-granular.
wasi_fd_read() {
  local __p=$1 __fd=$2 __iovs=$3 __iovs_len=$4 __nread_ptr=$5
  local -n __m=${__p}mem
  local -n __fds=${__p}wfds
  local -n __tell=${__p}wtell
  local __kind=${__fds[$__fd]-}
  if [[ -z $__kind ]]; then
    R0=8 # EBADF
    return 0
  fi
  if [[ $__kind == 3 ]]; then
    R0=31 # EISDIR
    return 0
  fi
  local LC_ALL=C
  local __i __j __ptr __len __total=0 __mk
  if [[ $__kind == 2 ]]; then
    local -n __wrbase=${__p}wrbase
    if (( (__wrbase[$__fd] & 0x2) == 0 )); then
      R0=76 # ENOTCAPABLE: fd lacks FD_READ
      return 0
    fi
    local -n __buf=${__p}wbuf${__fd}
    local __buflen=${#__buf[@]} __pos=${__tell[$__fd]}
    for (( __i = 0; __i < __iovs_len && __pos < __buflen; __i++ )); do
      mem_i32_load "$__p" $(( __iovs + __i * 8 )) || return $?
      __ptr=$R0
      mem_i32_load "$__p" $(( __iovs + __i * 8 + 4 )) || return $?
      __len=$R0
      if (( __len == 0 )); then continue; fi
      mem_check "$__p" "$__ptr" "$__len" || return $?
      for (( __j = 0; __j < __len && __pos < __buflen; __j++ )); do
        __mk=$(( __ptr + __j ))
        __m[$__mk]=$(( __buf[__pos] & 0xff ))
        (( __pos++, __total++ ))
      done
    done
    __tell[$__fd]=$__pos
    mem_i32_store "$__p" "$__nread_ptr" "$__total" || return $?
    R0=0
    return 0
  fi
  if (( __fd != 0 )); then
    R0=8 # EBADF: only stdin is readable
    return 0
  fi
  local -n __push=${__p}wpush
  local __ch __b __stop=0 __line __rc __k __tty=0
  [[ -t 0 ]] && __tty=1
  for (( __i = 0; __i < __iovs_len && __stop == 0; __i++ )); do
    mem_i32_load "$__p" $(( __iovs + __i * 8 )) || return $?
    __ptr=$R0
    mem_i32_load "$__p" $(( __iovs + __i * 8 + 4 )) || return $?
    __len=$R0
    if (( __len == 0 )); then continue; fi
    mem_check "$__p" "$__ptr" "$__len" || return $?
    for (( __j = 0; __j < __len; __j++ )); do
      if [[ -n $__push ]]; then
        __b=${__push%% *}
        if [[ $__push == *' '* ]]; then __push=${__push#* }; else __push=''; fi
      elif (( __tty )); then
        # Short-read handling as below: only the first byte of the call blocks.
        # Once the buffered line is drained, return what was delivered rather than blocking on the next line.
        if (( __total > 0 )); then
          __stop=1
          break
        fi
        IFS= read -r __line
        __rc=$?
        if (( __rc != 0 )) && [[ -z $__line ]]; then
          __stop=1 # EOF
          break
        fi
        for (( __k = 0; __k < ${#__line}; __k++ )); do
          printf -v __b '%d' "'${__line:__k:1}"
          __push+=${__push:+ }
          __push+=$__b
        done
        if (( __rc == 0 )); then
          # `read` strips the delimiter; restore it (rc != 0 with content is
          # EOF without a trailing newline).
          __push+=${__push:+ }
          __push+=10
        fi
        __b=${__push%% *}
        if [[ $__push == *' '* ]]; then __push=${__push#* }; else __push=''; fi
      else
        # Short-read handling: only the first byte of the call blocks; each further byte is taken only while input is already available.
        # `read
        # -t 0` reports readiness without consuming (success iff a byte is ready), giving the readpartial short-read semantics wasmtime/Ruby offer and that line-buffered tty guests (the QuickJS REPL) need: a full iovec is not drained past what one interactive line delivered.
        if (( __total > 0 )) && ! IFS= read -r -t 0; then
          __stop=1
          break
        fi
        if ! IFS= read -r -d '' -n 1 __ch; then
          __stop=1
          break
        elif [[ -z $__ch ]]; then
          __b=0
        else
          printf -v __b '%d' "'$__ch"
        fi
      fi
      __mk=$(( __ptr + __j ))
      __m[$__mk]=$(( __b & 0xff ))
      (( __total += 1 ))
    done
  done
  (( __tell[0] += __total ))
  mem_i32_store "$__p" "$__nread_ptr" "$__total" || return $?
  R0=0
  return 0
}
