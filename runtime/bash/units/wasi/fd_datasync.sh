# requires: wasi/fd_sync
# WASI fd_datasync: identical to fd_sync in this design.
# There is no separate data-only durability barrier since sync is just a flush.
wasi_fd_datasync() {
  local __p=$1 __fd=$2
  wasi_fd_sync "$__p" "$__fd" || return $?
  return 0
}
