# requires: wasi/read_path, wasi/resolve_path
# WASI path_symlink (ADR-40): create a symbolic link named <new_path> under the
# dirfd whose target string is <old_path>, written VERBATIM — the guest target
# is never resolved or made absolute, so a relative link stays relative and a
# dangling link is created as-is. This is a fifth namespace-mutation syscall
# licensed (beyond ADR-34 D2's four) to shell out to a single `--`-guarded
# `ln -s`; only the link's own parent is resolved with sandbox containment.
# A destination that already exists is EEXIST (20) — unless it is a
# slash-suffixed non-directory, which is ENOTDIR (54, ADR-49); a trailing
# slash on a not-yet-existing destination is ENOENT (44) — you cannot create
# a link "at a directory". The target string itself is not followed, so a
# file-symlink target does not hit the D3 pure-bash follow limitation.
wasi_path_symlink() {
  local __p=$1 __old_ptr=$2 __old_len=$3 __dirfd=$4 __new_ptr=$5 __new_len=$6
  wasi_read_path "$__p" "$__old_ptr" "$__old_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __target=$R1
  wasi_read_path "$__p" "$__new_ptr" "$__new_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __new=$R1
  local __trailing=0
  while [[ $__new == */ ]]; do __new=${__new%/}; __trailing=1; done
  wasi_resolve_path "$__p" "$__dirfd" "$__new" 0 || return $?
  if (( R0 != 0 )); then return 0; fi
  local __dest=$R1
  # A slash-suffixed link name over an existing *non*-directory is ENOTDIR
  # before the plain EEXIST (wasmtime's resolution order, ADR-49; the
  # upstream wasi-testsuite pins ENOTDIR under its strict errno modes).
  if (( __trailing )) && [[ ( -e $__dest || -h $__dest ) && ! -d $__dest ]]; then
    R0=54 # ENOTDIR
    return 0
  fi
  if [[ -e $__dest || -h $__dest ]]; then
    R0=20 # EEXIST
    return 0
  fi
  if (( __trailing )); then
    R0=44 # ENOENT: a trailing slash names a directory that does not exist
    return 0
  fi
  if command ln -s -- "$__target" "$__dest" 2>/dev/null; then
    R0=0
  else
    R0=29 # EIO
  fi
  return 0
}
