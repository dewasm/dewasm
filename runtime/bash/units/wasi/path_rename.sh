# requires: wasi/read_path, wasi/resolve_path
# WASI path_rename (ADR-34 D2): one of the four namespace-mutation units
# licensed to shell out — a single `--`-guarded `mv` on the two resolved
# physical paths. rename(2) never follows trailing symlinks on either side
# (it moves the link itself and replaces the destination link), so both
# resolutions use follow_last=0, mirroring
# runtime/ruby/units/wasi/path_rename.rb.
#
# `mv`'s own semantics diverge from rename(2) whenever the destination
# already exists, so the divergent cases are probed and handled *before*
# `mv` ever runs, rather than trusting `mv`'s exit status/diagnostics:
#   - missing source: ENOENT.
#   - destination exists, source is a directory, destination is not: ENOTDIR
#     (renaming a directory onto a non-directory).
#   - destination exists, destination is a directory, source is not: EISDIR
#     (renaming a file onto a directory).
#   - destination exists and both sides are directories: ENOTEMPTY, always —
#     not just when the destination is non-empty. `mv olddir newdir` moves
#     olddir *inside* newdir instead of replacing it when newdir already
#     exists (even empty), which `mv` alone cannot avoid without a second
#     command; rather than silently doing the wrong thing for an existing
#     empty destination directory, this backend refuses the whole
#     directory-onto-existing-directory case. A documented gap versus
#     rename(2)/Ruby, which do support replacing an empty destination
#     directory.
#   - destination exists, both sides are non-directories: falls through to
#     `mv`, which replaces the destination like rename(2) does.
# Anything else `mv` still manages to fail on (e.g. a permission error)
# defaults to EIO.
wasi_path_rename() {
  local __p=$1 __old_dirfd=$2 __old_path_ptr=$3 __old_path_len=$4
  local __new_dirfd=$5 __new_path_ptr=$6 __new_path_len=$7
  wasi_read_path "$__p" "$__old_path_ptr" "$__old_path_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __old_rel=$R1
  wasi_resolve_path "$__p" "$__old_dirfd" "$__old_rel" 0 || return $?
  if (( R0 != 0 )); then return 0; fi
  local __old_host=$R1
  wasi_read_path "$__p" "$__new_path_ptr" "$__new_path_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __new_rel=$R1
  wasi_resolve_path "$__p" "$__new_dirfd" "$__new_rel" 0 || return $?
  if (( R0 != 0 )); then return 0; fi
  local __new_host=$R1
  if [[ ! -e $__old_host ]]; then
    R0=44 # ENOENT
    return 0
  fi
  local __old_is_dir=0 __new_is_dir=0
  [[ -d $__old_host ]] && __old_is_dir=1
  if [[ -e $__new_host ]]; then
    [[ -d $__new_host ]] && __new_is_dir=1
    if (( __old_is_dir && ! __new_is_dir )); then
      R0=54 # ENOTDIR: renaming a directory onto a non-directory
      return 0
    fi
    if (( ! __old_is_dir && __new_is_dir )); then
      R0=31 # EISDIR: renaming a file onto a directory
      return 0
    fi
    if (( __old_is_dir && __new_is_dir )); then
      R0=55 # ENOTEMPTY: see header — mv can't replace an existing dir target
      return 0
    fi
  fi
  if command mv -- "$__old_host" "$__new_host" 2>/dev/null; then
    R0=0
  else
    R0=29 # EIO
  fi
  return 0
}
