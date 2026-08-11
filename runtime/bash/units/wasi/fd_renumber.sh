# requires: wasi/fd_flush
# WASI fd_renumber: atomically move the open fd <from> into the slot
# <to>, closing whatever <to> was.
# Both must currently be open: an unopened endpoint is EBADF (8).
# That is why `fd_renumber(valid, closed)` fails and why renumbering onto stdio or a preopen (both open) works.
# Every parallel fd-table slot (kind, offset, path, name, rights, fdflags, append, dirty) and, for a regular-file fd, the whole-file byte buffer moves from <from> to <to>;
# the destination's own buffer is flushed first (it is being closed) and any readdir cache on either side is dropped so a later fd_readdir rebuilds.
wasi_fd_renumber() {
  local __p=$1 __from=$2 __to=$3
  local -n __fds=${__p}wfds
  if [[ -z ${__fds[$__from]-} || -z ${__fds[$__to]-} ]]; then
    R0=8 # EBADF
    return 0
  fi
  if (( __from == __to )); then
    R0=0
    return 0
  fi
  local __kind=${__fds[$__from]}
  # Close the destination: flush a dirty buffer, then drop its per-fd state.
  wasi_fd_flush "$__p" "$__to" || return $?
  unset "${__p}wbuf${__to}" "${__p}wdn${__to}" "${__p}wdt${__to}"
  # Move every parallel slot; an absent source slot clears the destination's.
  local __a
  for __a in wfds wtell wpath wname wrbase wrinh wfdflags wapp wdirty; do
    local -n __arr=${__p}${__a}
    if [[ -v __arr[$__from] ]]; then
      __arr[$__to]=${__arr[$__from]}
      unset "__arr[$__from]"
    else
      unset "__arr[$__to]"
    fi
    unset -n __arr # release the nameref before rebinding next round
  done
  # Move the whole-file buffer for a regular-file fd.
  if [[ $__kind == 2 ]]; then
    declare -ga "${__p}wbuf${__to}=()"
    local -n __src=${__p}wbuf${__from}
    local -n __dst=${__p}wbuf${__to}
    __dst=("${__src[@]}")
    unset "${__p}wbuf${__from}"
  fi
  unset "${__p}wdn${__from}" "${__p}wdt${__from}"
  R0=0
  return 0
}
