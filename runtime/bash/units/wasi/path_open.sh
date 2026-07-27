# requires: mem/i32_store, wasi/read_path, wasi/resolve_path, wasi/file_slurp
# WASI path_open (ADR-32), mirroring runtime/ruby/units/wasi/path_open.rb.
# Resolves the guest path (lookupflags bit 0 = follow symlinks), then either
# opens a directory fd (O_DIRECTORY, or an existing directory even without it)
# or a file fd backed by a whole-file byte buffer. oflags: CREAT=0x1,
# DIRECTORY=0x2, EXCL=0x4, TRUNC=0x8; rights FD_READ=0x2 / FD_WRITE=0x40 pick the
# open mode; fdflags APPEND=0x1. A missing directory with O_DIRECTORY is ENOENT
# (44), not ENOTDIR (54) — guests branch on the difference. The opened fd is
# written to guest memory. R0 is the errno.
wasi_path_open() {
  local __p=$1 __dirfd=$2 __dirflags=$3 __path_ptr=$4 __path_len=$5
  local __oflags=$6 __rights=$7 __rights_inh=$8 __fdflags=$9 __opened_ptr=${10}
  wasi_read_path "$__p" "$__path_ptr" "$__path_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __rel=$R1
  wasi_resolve_path "$__p" "$__dirfd" "$__rel" "$(( __dirflags & 1 ))" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __host=$R1
  local -n __fds=${__p}wfds
  local -n __tell=${__p}wtell
  local -n __wpath=${__p}wpath
  local -n __wnext=${__p}wnext
  local __fd=$__wnext
  if (( __oflags & 0x2 )); then
    # O_DIRECTORY: distinguish missing (ENOENT) from not-a-directory (ENOTDIR).
    if [[ ! -e $__host ]]; then R0=44; return 0; fi
    if [[ ! -d $__host ]]; then R0=54; return 0; fi
    __fds[$__fd]=3
    __tell[$__fd]=0
    __wpath[$__fd]=$__host
  elif [[ -d $__host ]]; then
    # An existing directory opens as a dir fd even without O_DIRECTORY (Ruby).
    __fds[$__fd]=3
    __tell[$__fd]=0
    __wpath[$__fd]=$__host
  else
    local __creat=$(( (__oflags & 0x1) != 0 ? 1 : 0 ))
    local __excl=$(( (__oflags & 0x4) != 0 ? 1 : 0 ))
    local __trunc=$(( (__oflags & 0x8) != 0 ? 1 : 0 ))
    local __exists=0
    [[ -e $__host ]] && __exists=1
    if (( __creat && __excl && __exists )); then R0=20; return 0; fi # EEXIST
    if (( ! __exists && ! __creat )); then R0=44; return 0; fi        # ENOENT
    declare -ga "${__p}wbuf${__fd}=()"
    if (( __trunc || (__creat && ! __exists) )); then
      # Fresh or truncated: empty buffer, and create/truncate the file eagerly.
      if ! : > "$__host"; then R0=29; return 0; fi # EIO
    else
      wasi_file_slurp "$__host" "${__p}wbuf${__fd}" || return $?
      if (( R0 != 0 )); then return 0; fi
    fi
    local -n __wrd=${__p}wrd
    local -n __wwr=${__p}wwr
    local -n __wapp=${__p}wapp
    local -n __wdirty=${__p}wdirty
    __fds[$__fd]=2
    __tell[$__fd]=0
    __wpath[$__fd]=$__host
    __wrd[$__fd]=$(( (__rights & 0x2) != 0 ? 1 : 0 ))
    __wwr[$__fd]=$(( (__rights & 0x40) != 0 ? 1 : 0 ))
    __wapp[$__fd]=$(( (__fdflags & 0x1) != 0 ? 1 : 0 ))
    __wdirty[$__fd]=0
  fi
  (( __wnext++ ))
  mem_i32_store "$__p" "$__opened_ptr" "$__fd" || return $?
  R0=0
  return 0
}
