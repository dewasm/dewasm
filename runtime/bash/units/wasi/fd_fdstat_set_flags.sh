# WASI fd_fdstat_set_flags (ADR-40): set an fd's open fdflags (APPEND=0x1,
# DSYNC=0x2, NONBLOCK=0x4, RSYNC=0x8, SYNC=0x10). Flipping APPEND updates the
# <p>wapp array fd_write consults, so clearing it makes subsequent writes land
# at the seek offset again (the fd_flags_set conformance case). The other flags
# are stored (and reflected by fd_fdstat_get) but are no-ops under the
# flush-on-close model (ADR-34 D1). An unopened fd is EBADF (8).
wasi_fd_fdstat_set_flags() {
  local __p=$1 __fd=$2 __flags=$3
  local -n __fds=${__p}wfds
  if [[ -z ${__fds[$__fd]-} ]]; then
    R0=8 # EBADF
    return 0
  fi
  local -n __wfdflags=${__p}wfdflags
  local -n __wapp=${__p}wapp
  __wfdflags[$__fd]=$(( __flags & 0x1F ))
  __wapp[$__fd]=$(( (__flags & 0x1) != 0 ? 1 : 0 ))
  R0=0
  return 0
}
