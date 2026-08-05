# wasi_init_preopens <p>: register the standalone/library preopens from the
# ordered WASI_DIRS array (the bash analogue of Ruby's `preopens:` kwarg, ADR-34)
# and initialize the parallel fd-table arrays for this prefix. Each WASI_DIRS
# entry is 'HOST::GUEST' (no `::` means guest==host); the host is resolved
# physically via `cd -P` (a non-directory host through its parent), and a host
# path that does not exist is a loud init failure (nonzero return, so <p>init
# fails). Dir fds start at 3 (past stdio) and <p>wnext is left pointing past
# the last one. Called from <p>init whenever at least one WASI import actually
# fell back to a bundled unit (an embedder that supplied them all through
# IMPORTS/PROVIDERS never gets this state); an unset/empty WASI_DIRS just
# initializes the arrays.
#
# Per-fd rights (ADR-40): <p>wrbase / <p>wrinh hold the u64 rights masks a fd
# exposes through fd_fdstat_get and enforces on fd_read/write/seek/readdir/
# filestat_set_size; <p>wfdflags holds the u16 open fdflags. stdio gets all-ones
# (a char device the guest may freely use). A preopen directory gets the
# canonical WASI directory rights: base = the directory-applicable set
# (0x7BFFE98 — the PATH_* ops plus FD_READDIR/FD_FILESTAT_GET/... but *not* the
# regular-file ops like FD_READ/FD_SEEK/FD_FILESTAT_SET_SIZE), inheriting =
# base | the regular-file base (0xFFFFFFF, every p1 right below SOCK_*). A file
# fd's masks are narrowed from these at path_open time.
wasi_init_preopens() {
  local __p=$1
  local -n __fds=${__p}wfds
  local -n __tell=${__p}wtell
  local -n __wnext=${__p}wnext
  declare -ga "${__p}wpath=()" "${__p}wname=()" "${__p}wapp=()" \
    "${__p}wdirty=()" "${__p}wrbase=()" "${__p}wrinh=()" "${__p}wfdflags=()"
  local -n __wpath=${__p}wpath
  local -n __wname=${__p}wname
  local -n __wrbase=${__p}wrbase
  local -n __wrinh=${__p}wrinh
  local -n __wfdflags=${__p}wfdflags
  local __sfd
  for __sfd in 0 1 2; do
    __wrbase[$__sfd]=-1
    __wrinh[$__sfd]=-1
    __wfdflags[$__sfd]=0
  done
  local __i __spec __host __guest __real __hostdir __fd=3
  for (( __i = 0; __i < ${#WASI_DIRS[@]}; __i++ )); do
    __spec=${WASI_DIRS[__i]}
    if [[ $__spec == *"::"* ]]; then
      __host=${__spec%%::*}
      __guest=${__spec#*::}
    else
      __host=$__spec
      __guest=$__spec
    fi
    # The host path must resolve, but need not be a directory: like the
    # Ruby/Perl runtimes, a single-file preopen (e.g. "/dev/null" for the
    # zeroperl reactor's init probe) is accepted — the guest resolves it as the
    # preopen root itself. A directory is resolved physically by entering it;
    # anything else by resolving its parent and re-attaching the basename.
    if [[ -d $__host ]]; then
      __real=$(cd -P -- "$__host" 2>/dev/null && pwd -P)
    elif [[ -e $__host ]]; then
      __hostdir=${__host%/*}
      [[ $__hostdir == "$__host" ]] && __hostdir=.
      [[ -z $__hostdir ]] && __hostdir=/
      __real=$(cd -P -- "$__hostdir" 2>/dev/null && pwd -P)
      [[ -n $__real ]] && __real=${__real%/}/${__host##*/}
    else
      __real=
    fi
    if [[ -z $__real ]]; then
      echo "preopen ${__guest}: cannot resolve host path ${__host}" >&2
      return 1
    fi
    __fds[$__fd]=3
    __tell[$__fd]=0
    __wpath[$__fd]=$__real
    __wname[$__fd]=$__guest
    __wrbase[$__fd]=$(( 0x7BFFE98 ))  # RIGHTS_DIRECTORY_BASE
    __wrinh[$__fd]=$(( 0xFFFFFFF ))   # RIGHTS_DIRECTORY_INHERITING
    __wfdflags[$__fd]=0
    (( __fd++ ))
  done
  __wnext=$__fd
  return 0
}
