# WASI fd_fdstat_set_rights: narrow an fd's stored rights.
# Rights can only be dropped, never regained: any requested bit not already held (in base or inheriting) is ENOTCAPABLE (76); an equal-or-narrower set is applied and returns success (which is why `supports_rights`, re-granting the current set, reports the backend as rights-supporting).
# An unopened fd is EBADF (8).
wasi_fd_fdstat_set_rights() {
  local __p=$1 __fd=$2 __base=$3 __inheriting=$4
  local -n __fds=${__p}wfds
  if [[ -z ${__fds[$__fd]-} ]]; then
    R0=8 # EBADF
    return 0
  fi
  local -n __wrbase=${__p}wrbase
  local -n __wrinh=${__p}wrinh
  local __old_base=${__wrbase[$__fd]} __old_inh=${__wrinh[$__fd]}
  if (( (__base & ~__old_base) != 0 || (__inheriting & ~__old_inh) != 0 )); then
    R0=76 # ENOTCAPABLE: cannot widen rights
    return 0
  fi
  __wrbase[$__fd]=$__base
  __wrinh[$__fd]=$__inheriting
  R0=0
  return 0
}
