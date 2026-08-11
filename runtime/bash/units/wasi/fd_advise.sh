# WASI fd_advise: advisory only.
# The whole-file buffer model has no readahead/cache to tune, so this validates the fd and returns success
# (ERRNO_SUCCESS), the same no-op a host is free to give.
# An unopened fd is
# EBADF (8).
wasi_fd_advise() {
  local __p=$1 __fd=$2 __offset=$3 __len=$4 __advice=$5
  local -n __fds=${__p}wfds
  if [[ -z ${__fds[$__fd]-} ]]; then
    R0=8 # EBADF
    return 0
  fi
  R0=0
  return 0
}
