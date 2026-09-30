# requires: wasi/read_path, wasi/resolve_path
# WASI `path_unlink_file`: one of the four namespace-mutation units licensed to shell out.
# It runs a single `--`-guarded `rm` (never `-r`) on the resolved physical path.
# unlink(2) never follows a trailing symbolic link, since it removes the link itself.
# So resolution uses `follow_last=0`.
# This mirrors `crates/dewasm-backend-ruby/units/wasi/path_unlink_file.rb`.
# A directory target is rejected up front with the host-split `errno` Wasmtime inherits.
# That `errno` is EPERM on macOS, EISDIR on Linux.
# A missing path is ENOENT; any other `rm` failure defaults to EIO.
wasi_path_unlink_file() {
  local __p=$1 __dirfd=$2 __path_ptr=$3 __path_len=$4
  wasi_read_path "$__p" "$__path_ptr" "$__path_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __rel=$R1
  wasi_resolve_path "$__p" "$__dirfd" "$__rel" 0 || return $?
  if (( R0 != 0 )); then return 0; fi
  local __host=$R1
  if [[ ! -e $__host && ! -h $__host ]]; then
    R0=44 # ENOENT
    return 0
  fi
  # A symbolic link is unlinked as the link itself (`-h` guards `-d`).
  # A real directory is rejected with the host-split `errno` Wasmtime inherits.
  # That `errno` is EPERM on macOS, EISDIR on Linux.
  if [[ -d $__host && ! -h $__host ]]; then
    if [[ $OSTYPE == darwin* ]]; then
      R0=63 # EPERM
    else
      R0=31 # EISDIR
    fi
    return 0
  fi
  if command rm -- "$__host" 2>/dev/null; then
    R0=0
  else
    R0=29 # EIO
  fi
  return 0
}
