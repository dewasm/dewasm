# requires: wasi/read_path, wasi/resolve_path
# WASI `path_rename`: one of the four namespace-mutation units licensed to shell out.
# It runs a single `--`-guarded `mv` on the two resolved physical paths.
# rename(2) never follows trailing symbolic links on either side.
# It moves the link itself and replaces the destination link.
# So both resolutions use `follow_last=0`.
# This mirrors `crates/dewasm-backend-ruby/units/wasi/path_rename.rb`.
#
# `mv`'s own semantics differ from rename(2) whenever the destination already exists.
# So the differing cases are probed and handled *before* `mv` ever runs.
# `mv`'s exit status/diagnostics are not trusted for them:
# - missing source: ENOENT.
# - destination exists, source is a directory, destination is not: ENOTDIR.
#   This is renaming a directory onto a non-directory.
# - destination exists, destination is a directory, source is not: EISDIR.
#   This is renaming a file onto a directory.
# - destination exists and both sides are directories:
#   rename(2) replaces an *empty* destination directory atomically.
#   `mv olddir newdir` instead moves `olddir` *inside* `newdir` when `newdir` already exists.
#   To match rename(2), an existing empty destination directory is `rmdir`'d first.
#   That stays within the `mkdir`/`rmdir`/`rm`/`mv` license.
#   The `mv` then renames onto the freed name.
#   A *non-empty* destination directory is ENOTEMPTY, since it cannot be replaced.
# - destination exists, both sides are non-directories: falls through to `mv`.
#   `mv` replaces the destination like rename(2) does.
# Trailing slashes on either name are stripped before resolution but remembered:
# - a slash-suffixed existing non-directory is ENOTDIR on either side;
# - a nonexistent slash-suffixed source is ENOENT;
# - a nonexistent slash-suffixed destination just loses the slash, and the rename proceeds.
#   Wasmtime does the same.
# Anything else `mv` fails on defaults to EIO.
wasi_path_rename() {
  local __p=$1 __old_dirfd=$2 __old_path_ptr=$3 __old_path_len=$4
  local __new_dirfd=$5 __new_path_ptr=$6 __new_path_len=$7
  wasi_read_path "$__p" "$__old_path_ptr" "$__old_path_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __old_rel=$R1 __old_slash=0
  [[ $__old_rel == */ ]] && __old_slash=1
  while [[ $__old_rel == */ ]]; do __old_rel=${__old_rel%/}; done
  wasi_resolve_path "$__p" "$__old_dirfd" "$__old_rel" 0 || return $?
  if (( R0 != 0 )); then return 0; fi
  local __old_host=$R1
  wasi_read_path "$__p" "$__new_path_ptr" "$__new_path_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __new_rel=$R1 __new_slash=0
  [[ $__new_rel == */ ]] && __new_slash=1
  while [[ $__new_rel == */ ]]; do __new_rel=${__new_rel%/}; done
  wasi_resolve_path "$__p" "$__new_dirfd" "$__new_rel" 0 || return $?
  if (( R0 != 0 )); then return 0; fi
  local __new_host=$R1
  if [[ ! -e $__old_host && ! -h $__old_host ]]; then
    R0=44 # ENOENT
    return 0
  fi
  local __old_is_dir=0 __new_is_dir=0
  [[ -d $__old_host ]] && __old_is_dir=1
  if (( __old_slash && ! __old_is_dir )); then
    R0=54 # ENOTDIR: trailing slash on a non-directory source
    return 0
  fi
  if [[ -e $__new_host || -h $__new_host ]]; then
    [[ -d $__new_host ]] && __new_is_dir=1
    if (( __new_slash && ! __new_is_dir )); then
      R0=54 # ENOTDIR: trailing slash on a non-directory destination
      return 0
    fi
    if (( __old_is_dir && ! __new_is_dir )); then
      R0=54 # ENOTDIR: renaming a directory onto a non-directory
      return 0
    fi
    if (( ! __old_is_dir && __new_is_dir )); then
      R0=31 # EISDIR: renaming a file onto a directory
      return 0
    fi
    if (( __old_is_dir && __new_is_dir )); then
      # Replace an empty destination directory (rename(2) semantics); refuse a non-empty one.
      local __save
      __save=$(shopt -p dotglob nullglob)
      shopt -s dotglob nullglob
      local __entries=("$__new_host"/*)
      eval "$__save"
      if (( ${#__entries[@]} > 0 )); then
        R0=55 # ENOTEMPTY
        return 0
      fi
      if ! command rmdir -- "$__new_host" 2>/dev/null; then
        # The destination survived.
        # So the `mv` below would nest the source inside it instead of replacing it.
        # Report the failure.
        # Re-probe emptiness: an entry that appeared since the check above is rename(2)'s ENOTEMPTY.
        # Anything else (for example a permission error on the parent) is the unit's default EIO.
        __save=$(shopt -p dotglob nullglob)
        shopt -s dotglob nullglob
        __entries=("$__new_host"/*)
        eval "$__save"
        if (( ${#__entries[@]} > 0 )); then
          R0=55 # ENOTEMPTY
        else
          R0=29 # EIO
        fi
        return 0
      fi
    fi
  fi
  if command mv -- "$__old_host" "$__new_host" 2>/dev/null; then
    R0=0
  else
    R0=29 # EIO
  fi
  return 0
}
