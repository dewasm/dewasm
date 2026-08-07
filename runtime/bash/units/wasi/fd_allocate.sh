# WASI fd_allocate: ensure the file is at least offset+len bytes,
# extending the whole-file byte buffer with zero bytes and marking
# it dirty; a request that does not grow past the current size is a no-op.
# Purely in-memory — no external command, unlike the namespace-mutation four.
# Only a regular-file fd (kind 2) can be allocated; a directory or stdio fd is
# EBADF (8), which dir_fd_op_failures accepts alongside ISDIR/NOTCAPABLE.
wasi_fd_allocate() {
  local __p=$1 __fd=$2 __offset=$3 __len=$4
  local -n __fds=${__p}wfds
  if [[ ${__fds[$__fd]-} != 2 ]]; then
    R0=8 # EBADF
    return 0
  fi
  if (( __offset < 0 || __len < 0 )); then
    R0=28 # EINVAL
    return 0
  fi
  if (( __len > 0x7fffffffffffffff - __offset )); then
    # offset+len would wrap bash's signed-64 arithmetic; no file can be that
    # large. EIO matches Ruby/Python, whose OS-level EFBIG falls through their
    # generic errno mapping as ERRNO_IO.
    R0=29 # EIO
    return 0
  fi
  local __want=$(( __offset + __len ))
  local -n __wbuf=${__p}wbuf${__fd}
  local -n __wdirty=${__p}wdirty
  local __cur=${#__wbuf[@]} __i
  if (( __want > __cur )); then
    for (( __i = __cur; __i < __want; __i++ )); do
      __wbuf[__i]=0
    done
    __wdirty[$__fd]=1
  fi
  R0=0
  return 0
}
