# requires: mem/i32_store, wasi/read_path, wasi/resolve_path, wasi/file_slurp
# WASI path_open (ADR-34, rights in ADR-40), mirroring
# runtime/ruby/units/wasi/path_open.rb. Resolves the guest path (lookupflags
# bit 0 = follow symlinks), then opens a directory or a whole-file byte-buffer
# fd. oflags: CREAT=0x1, DIRECTORY=0x2, EXCL=0x4, TRUNC=0x8; rights FD_READ=0x2
# / FD_WRITE=0x40 pick the open mode; fdflags APPEND=0x1.
#
# Rights model: the dirfd must hold PATH_OPEN (0x2000) or the open is
# NOTCAPABLE (76); O_TRUNC additionally needs PATH_FILESTAT_SET_SIZE (0x80000)
# on the dirfd. The opened fd's granted base = requested & dirfd-inheriting &
# the per-filetype mask (directory 0x7BFFE98 / regular-file 0x8E001FF); its
# inheriting = requested-inheriting & dirfd-inheriting & (directory 0xFFFFFFF /
# empty for a file). Opening a directory with FD_WRITE requested is EISDIR (31).
# A missing directory with O_DIRECTORY is ENOENT (44), not ENOTDIR (54).
# A symlink final component with NOFOLLOW (lookupflags bit 0 clear) is ELOOP
# (32) — you cannot open the link itself (ADR-34 D3). The opened fd is written
# to guest memory. R0 is the errno.
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
  local -n __wrbase=${__p}wrbase
  local -n __wrinh=${__p}wrinh
  local -n __wfdflags=${__p}wfdflags
  local __dir_base=${__wrbase[$__dirfd]}
  local __dir_inh=${__wrinh[$__dirfd]}
  if (( (__dir_base & 0x2000) == 0 )); then R0=76; return 0; fi          # NOTCAPABLE: no PATH_OPEN
  if (( (__oflags & 0x8) && (__dir_base & 0x80000) == 0 )); then R0=76; return 0; fi # TRUNC needs PATH_FILESTAT_SET_SIZE
  # A symlink final component that was not followed cannot be opened (D3).
  if (( (__dirflags & 1) == 0 )) && [[ -h $__host ]]; then R0=32; return 0; fi
  # A slash-suffixed name may only resolve to a directory (issue #42); an
  # existing non-directory was already ENOTDIR in resolve_path, and the create
  # branch below must never manufacture a plain *file* through the slash. The
  # nonexistent case follows wasmtime 47 (ADR-49): with O_CREAT it is EINVAL
  # on macOS (wasmtime's manual path resolution rejects the shape) and EISDIR
  # on Linux (host passthrough); a plain open is ENOENT on both.
  if [[ $__rel == */ && ! -e $__host && ! -h $__host ]]; then
    if (( __oflags & 0x1 )); then
      if [[ $OSTYPE == darwin* ]]; then
        R0=28 # EINVAL
      else
        R0=31 # EISDIR
      fi
    else
      R0=44 # ENOENT
    fi
    return 0
  fi
  local __fd=$__wnext
  if (( __oflags & 0x2 )) || { [[ ! -h $__host ]] && [[ -d $__host ]]; }; then
    if (( __oflags & 0x2 )); then
      # O_DIRECTORY: distinguish missing (ENOENT) from not-a-directory (ENOTDIR).
      if [[ ! -e $__host ]]; then R0=44; return 0; fi
      if [[ ! -d $__host ]]; then R0=54; return 0; fi
    fi
    # Opening a directory for writing is EISDIR.
    if (( __rights & 0x40 )); then R0=31; return 0; fi
    __fds[$__fd]=3
    __tell[$__fd]=0
    __wpath[$__fd]=$__host
    __wrbase[$__fd]=$(( __rights & __dir_inh & 0x7BFFE98 ))
    __wrinh[$__fd]=$(( __rights_inh & __dir_inh & 0xFFFFFFF ))
    __wfdflags[$__fd]=$(( __fdflags & 0x1F ))
  else
    local __creat=$(( (__oflags & 0x1) != 0 ? 1 : 0 ))
    local __excl=$(( (__oflags & 0x4) != 0 ? 1 : 0 ))
    local __trunc=$(( (__oflags & 0x8) != 0 ? 1 : 0 ))
    local __exists=0
    [[ -e $__host || -h $__host ]] && __exists=1
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
    local -n __wapp=${__p}wapp
    local -n __wdirty=${__p}wdirty
    __fds[$__fd]=2
    __tell[$__fd]=0
    __wpath[$__fd]=$__host
    __wrbase[$__fd]=$(( __rights & __dir_inh & 0x8E001FF ))
    __wrinh[$__fd]=0
    __wfdflags[$__fd]=$(( __fdflags & 0x1F ))
    __wapp[$__fd]=$(( (__fdflags & 0x1) != 0 ? 1 : 0 ))
    __wdirty[$__fd]=0
  fi
  (( __wnext++ ))
  mem_i32_store "$__p" "$__opened_ptr" "$__fd" || return $?
  R0=0
  return 0
}
