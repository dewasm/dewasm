# wasi_init_preopens <p>: register the standalone/library preopens from the
# ordered WASI_DIRS array (the bash analogue of Ruby's `preopens:` kwarg, ADR-32)
# and initialize the parallel fd-table arrays for this prefix. Each WASI_DIRS
# entry is 'HOST::GUEST' (no `::` means guest==host); the host is resolved
# physically via `cd -P`, and an unresolvable host is a loud init failure
# (nonzero return, so <p>init fails). Dir fds start at 3 (past stdio) and
# <p>wnext is left pointing past the last one. Always called from <p>init when
# WASI is bundled; an unset/empty WASI_DIRS just initializes the arrays.
wasi_init_preopens() {
  local __p=$1
  local -n __fds=${__p}wfds
  local -n __tell=${__p}wtell
  local -n __wnext=${__p}wnext
  declare -ga "${__p}wpath=()" "${__p}wname=()" "${__p}wrd=()" \
    "${__p}wwr=()" "${__p}wapp=()" "${__p}wdirty=()"
  local -n __wpath=${__p}wpath
  local -n __wname=${__p}wname
  local __i __spec __host __guest __real __fd=3
  for (( __i = 0; __i < ${#WASI_DIRS[@]}; __i++ )); do
    __spec=${WASI_DIRS[__i]}
    if [[ $__spec == *"::"* ]]; then
      __host=${__spec%%::*}
      __guest=${__spec#*::}
    else
      __host=$__spec
      __guest=$__spec
    fi
    __real=$(cd -P -- "$__host" 2>/dev/null && pwd -P)
    if [[ -z $__real ]]; then
      echo "preopen ${__guest}: cannot resolve host directory ${__host}" >&2
      return 1
    fi
    __fds[$__fd]=3
    __tell[$__fd]=0
    __wpath[$__fd]=$__real
    __wname[$__fd]=$__guest
    (( __fd++ ))
  done
  __wnext=$__fd
  return 0
}
