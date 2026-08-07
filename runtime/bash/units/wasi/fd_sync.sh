# requires: wasi/fd_flush
# WASI fd_sync: sync is treated as flush — the whole-file-buffer
# model has no separate durability barrier. Delegates to wasi_fd_flush, which
# is already a no-op (R0=0) for a directory/stdio fd or a clean buffer; only a
# genuinely unknown fd is rejected here (every other fd_* unit checks this).
wasi_fd_sync() {
  local __p=$1 __fd=$2
  local -n __fds=${__p}wfds
  if [[ -z ${__fds[$__fd]-} ]]; then
    R0=8 # EBADF
    return 0
  fi
  wasi_fd_flush "$__p" "$__fd" || return $?
  return 0
}
