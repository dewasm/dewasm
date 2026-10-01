# requires: wasi/read_path, wasi/resolve_path
# WASI `path_create_directory`: creating a directory entry is one of four operations.
# Pure Bash cannot express any of the four.
# So this is one of the only four units allowed to run an external POSIX command.
# That command is a single `--`-guarded `mkdir` on the resolved physical path.
# `mkdir(2)` never follows a trailing symbolic link: an existing one is EEXIST, not "enter it".
# So resolution uses `follow_last=0`.
# This mirrors `crates/dewasm-backend-ruby/units/wasi/path_create_directory.rb`.
# The diagnostics of `mkdir` aren't parsed, since they're not machine-readable.
# The `errno` comes from a probe afterwards instead.
# An existing path is EEXIST, and anything else defaults to ENOENT.
# A parent-permission failure would also surface as ENOENT here.
# That is an accepted loss of precision from probing afterwards instead of reading the real `errno`.
wasi_path_create_directory() {
  local __p=$1 __dirfd=$2 __path_ptr=$3 __path_len=$4
  wasi_read_path "$__p" "$__path_ptr" "$__path_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __rel=$R1
  # Strip a trailing slash before the resolver's directory check.
  # `mkdir` names a directory anyway.
  # EEXIST is Wasmtime's answer for `mkdir("file/")` where the hosts split.
  local __stripped=$__rel
  while [[ $__stripped == */ ]]; do __stripped=${__stripped%/}; done
  [[ -n $__stripped ]] && __rel=$__stripped
  wasi_resolve_path "$__p" "$__dirfd" "$__rel" 0 || return $?
  if (( R0 != 0 )); then return 0; fi
  local __host=$R1
  if command mkdir -- "$__host" 2>/dev/null; then
    R0=0
    return 0
  fi
  if [[ -e $__host ]]; then
    R0=20 # EEXIST
  else
    R0=44 # ENOENT
  fi
  return 0
}
