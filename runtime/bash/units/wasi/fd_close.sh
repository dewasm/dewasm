# requires: wasi/fd_flush
# Closing a file fd (kind 2) flushes a dirty buffer to disk, then drops the
# buffer and its parallel fd-table entries (ADR-34). fds are never reused. A
# directory or stdio fd just leaves the table.
wasi_fd_close() {
  local __p=$1 __fd=$2
  local -n __fds=${__p}wfds
  local __kind=${__fds[$__fd]-}
  if [[ -z $__kind ]]; then
    R0=8 # EBADF
    return 0
  fi
  if [[ $__kind == 2 ]]; then
    wasi_fd_flush "$__p" "$__fd" || return $?
    unset "${__p}wbuf${__fd}"
    local -n __wpath=${__p}wpath
    local -n __wrd=${__p}wrd
    local -n __wwr=${__p}wwr
    local -n __wapp=${__p}wapp
    local -n __wdirty=${__p}wdirty
    local -n __tell=${__p}wtell
    unset "__wpath[$__fd]" "__wrd[$__fd]" "__wwr[$__fd]" \
      "__wapp[$__fd]" "__wdirty[$__fd]" "__tell[$__fd]"
  fi
  unset "__fds[$__fd]"
  R0=0
  return 0
}
