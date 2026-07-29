# requires: wasi/read_path, wasi/resolve_path
# WASI path_unlink_file (ADR-34 D2): one of the four namespace-mutation units
# licensed to shell out — a single `--`-guarded `rm` (never `-r`) on the
# resolved physical path. unlink(2) never follows a trailing symlink (it
# removes the link itself), so resolution uses follow_last=0, mirroring
# runtime/ruby/units/wasi/path_unlink_file.rb. A directory target is rejected
# up front (before ever invoking `rm`) with the errno wasmtime inherits from
# the host unlink(2) — EPERM on macOS, EISDIR on Linux (ADR-49): this backend
# never calls the OS unlink(2) itself (it probes `-d` first), so it emulates
# the host split the upstream wasi-testsuite pins under its strict errno
# modes. A missing path is ENOENT; any other `rm` failure (e.g. a permission
# error) defaults to EIO.
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
  # A symlink (even to a directory, even dangling) is unlinked as the link
  # itself; only a real directory is rejected (`-h` guards the `-d` test,
  # which would otherwise dereference a symlink-to-directory). The errno
  # mirrors what wasmtime inherits from the host unlink(2) (ADR-49): EPERM
  # on macOS, EISDIR on Linux — the upstream wasi-testsuite pins exactly
  # that split under its strict errno modes, so the one-value pick the unit
  # header used to document is gone.
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
