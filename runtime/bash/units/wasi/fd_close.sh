# requires: wasi/fd_flush
# Closing a file fd (kind 2) flushes a dirty buffer to disk, then drops the
# buffer and its parallel fd-table entries. fds are never reused. A
# directory or stdio fd just leaves the table. A flush failure's errno (R0,
# the only channel wasi_fd_flush reports through) is propagated so the guest
# learns its writes were lost, but the fd state is released either way: the
# fd is gone after close, success or not.
wasi_fd_close() {
  local __p=$1 __fd=$2
  local -n __fds=${__p}wfds
  local __kind=${__fds[$__fd]-}
  if [[ -z $__kind ]]; then
    R0=8 # EBADF
    return 0
  fi
  local __errno=0
  if [[ $__kind == 2 ]]; then
    wasi_fd_flush "$__p" "$__fd" || return $?
    __errno=$R0
    unset "${__p}wbuf${__fd}"
    local -n __wpath=${__p}wpath
    local -n __wrbase=${__p}wrbase
    local -n __wrinh=${__p}wrinh
    local -n __wfdflags=${__p}wfdflags
    local -n __wapp=${__p}wapp
    local -n __wdirty=${__p}wdirty
    local -n __tell=${__p}wtell
    unset "__wpath[$__fd]" "__wrbase[$__fd]" "__wrinh[$__fd]" \
      "__wfdflags[$__fd]" "__wapp[$__fd]" "__wdirty[$__fd]" "__tell[$__fd]"
  fi
  unset "__fds[$__fd]"
  R0=$__errno
  return 0
}
