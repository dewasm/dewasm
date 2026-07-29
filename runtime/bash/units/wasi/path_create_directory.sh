# requires: wasi/read_path, wasi/resolve_path
# WASI path_create_directory (ADR-34 D2): creating a directory entry is one
# of the four operations pure Bash cannot express at all, so this is one of
# the only four units allowed to invoke an external POSIX command — a single
# `--`-guarded `mkdir` on the resolved physical path. mkdir(2) never follows a
# trailing symlink (an existing one there is EEXIST, not "enter it"), so
# resolution uses follow_last=0, mirroring
# runtime/ruby/units/wasi/path_create_directory.rb. mkdir's own diagnostics
# aren't parsed (they're not machine-readable); the errno comes from a
# post-hoc probe instead: an existing path is EEXIST, anything else defaults
# to ENOENT (a parent-permission failure would also surface as ENOENT here —
# an accepted imprecision of probing after the fact rather than reading the
# real errno).
wasi_path_create_directory() {
  local __p=$1 __dirfd=$2 __path_ptr=$3 __path_len=$4
  wasi_read_path "$__p" "$__path_ptr" "$__path_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __rel=$R1
  # Strip a trailing slash before the resolver's directory gate: mkdir names
  # a directory anyway, and EEXIST is wasmtime's answer for mkdir("file/")
  # where the hosts split (ADR-49).
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
