# requires: wasi/read_path, wasi/resolve_path
# WASI path_remove_directory (ADR-34 D2): one of the four namespace-mutation
# units licensed to shell out — a single `--`-guarded `rmdir` on the resolved
# physical path. rmdir(2) never follows a trailing symlink, so resolution
# uses follow_last=0, mirroring
# runtime/ruby/units/wasi/path_remove_directory.rb. rmdir's own diagnostics
# aren't parsed; the errno comes from post-hoc probes in a fixed order:
# missing is ENOENT, existing-but-not-a-directory is ENOTDIR, an existing
# non-empty directory is ENOTEMPTY (probed with the same dotglob+nullglob
# snapshot-and-restore idiom fd_readdir uses), and anything else left over
# (e.g. a permission failure on an apparently-empty directory) defaults to
# EIO.
wasi_path_remove_directory() {
  local __p=$1 __dirfd=$2 __path_ptr=$3 __path_len=$4
  wasi_read_path "$__p" "$__path_ptr" "$__path_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __rel=$R1
  wasi_resolve_path "$__p" "$__dirfd" "$__rel" 0 || return $?
  if (( R0 != 0 )); then return 0; fi
  local __host=$R1
  if command rmdir -- "$__host" 2>/dev/null; then
    R0=0
    return 0
  fi
  if [[ ! -e $__host ]]; then
    R0=44 # ENOENT
  elif [[ ! -d $__host ]]; then
    R0=54 # ENOTDIR
  else
    local __save
    __save=$(shopt -p dotglob nullglob)
    shopt -s dotglob nullglob
    local __entries=("$__host"/*)
    eval "$__save"
    if (( ${#__entries[@]} > 0 )); then
      R0=55 # ENOTEMPTY
    else
      R0=29 # EIO: apparently empty and a directory, rmdir still failed
    fi
  fi
  return 0
}
