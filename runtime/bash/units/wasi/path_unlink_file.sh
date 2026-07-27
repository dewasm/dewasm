# requires: wasi/read_path, wasi/resolve_path
# WASI path_unlink_file (ADR-32 D2): one of the four namespace-mutation units
# licensed to shell out — a single `--`-guarded `rm` (never `-r`) on the
# resolved physical path. unlink(2) never follows a trailing symlink (it
# removes the link itself), so resolution uses follow_last=0, mirroring
# runtime/ruby/units/wasi/path_unlink_file.rb. A directory target is rejected
# up front (before ever invoking `rm`) as EISDIR: this is the errno Linux's
# unlink(2) raises for a directory and what most WASI runtimes (wasmtime
# included, on the Linux hosts they mostly run on) surface — a native macOS
# unlink(2) would instead raise EPERM, the same cross-platform split Ruby's
# FS_ERRNO table already accepts both sides of, but this backend never calls
# the OS unlink(2) itself (it probes `-d` first), so it picks the one value
# rather than inheriting whichever the host OS happens to raise. A missing
# path is ENOENT; any other `rm` failure (e.g. a permission error) defaults
# to EIO.
wasi_path_unlink_file() {
  local __p=$1 __dirfd=$2 __path_ptr=$3 __path_len=$4
  wasi_read_path "$__p" "$__path_ptr" "$__path_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __rel=$R1
  wasi_resolve_path "$__p" "$__dirfd" "$__rel" 0 || return $?
  if (( R0 != 0 )); then return 0; fi
  local __host=$R1
  if [[ ! -e $__host ]]; then
    R0=44 # ENOENT
    return 0
  fi
  if [[ -d $__host ]]; then
    R0=31 # EISDIR
    return 0
  fi
  if command rm -- "$__host" 2>/dev/null; then
    R0=0
  else
    R0=29 # EIO
  fi
  return 0
}
