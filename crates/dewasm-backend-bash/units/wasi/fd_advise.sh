# WASI `fd_advise`: the advice is only a hint.
# The whole-file buffer model has no cache and no reading ahead to tune.
# So this validates the `fd` and returns success (ERRNO_SUCCESS).
# That is the same no-op a host is free to give.
# An unopened `fd` is EBADF (8).
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
