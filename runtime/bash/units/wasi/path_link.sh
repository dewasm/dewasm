# requires: wasi/read_path, wasi/resolve_path
# WASI path_link: create a hard link <new_path> under <new_fd> to the
# existing <old_path> under <old_fd>. A licensed `--`-guarded `ln -P` (beyond
# the four mkdir/rmdir/rm/mv commands) — `-P` hard-links a symlink source
# itself rather than its pointee, so a link to a dangling or looping symlink is
# created without ever following it. Both endpoints are resolved with sandbox
# containment, NOFOLLOW (hard-linking never dereferences a trailing symlink).
# Symlink-following (LOOKUPFLAGS_SYMLINK_FOLLOW on old_flags) is rejected with
# EINVAL (28). A missing source is ENOENT (44); a directory source is EPERM
# (63); an existing destination is EEXIST (20); a destination with a trailing
# slash is ENOENT (44).
wasi_path_link() {
  local __p=$1 __old_fd=$2 __old_flags=$3 __old_ptr=$4 __old_len=$5
  local __new_fd=$6 __new_ptr=$7 __new_len=$8
  if (( __old_flags & 1 )); then
    R0=28 # EINVAL: hard-linking with symlink-follow is not supported
    return 0
  fi
  wasi_read_path "$__p" "$__old_ptr" "$__old_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __old_rel=$R1
  wasi_resolve_path "$__p" "$__old_fd" "$__old_rel" 0 || return $?
  if (( R0 != 0 )); then return 0; fi
  local __src=$R1
  wasi_read_path "$__p" "$__new_ptr" "$__new_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __new_rel=$R1
  local __trailing=0
  while [[ $__new_rel == */ ]]; do __new_rel=${__new_rel%/}; __trailing=1; done
  wasi_resolve_path "$__p" "$__new_fd" "$__new_rel" 0 || return $?
  if (( R0 != 0 )); then return 0; fi
  local __dst=$R1
  if [[ ! -e $__src && ! -h $__src ]]; then
    R0=44 # ENOENT: missing source
    return 0
  fi
  if [[ -d $__src && ! -h $__src ]]; then
    R0=63 # EPERM: cannot hard-link a directory
    return 0
  fi
  if [[ -e $__dst || -h $__dst ]]; then
    R0=20 # EEXIST
    return 0
  fi
  if (( __trailing )); then
    R0=44 # ENOENT: a trailing slash names a directory that does not exist
    return 0
  fi
  if command ln -P -- "$__src" "$__dst" 2>/dev/null; then
    R0=0
  else
    R0=29 # EIO
  fi
  return 0
}
