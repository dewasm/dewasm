# wasi_resolve_path <p> <dirfd> <rel> <follow>: resolve a guest-relative path
# against a directory fd to a physical host path, confined to that dirfd's own
# stored root (ADR-32, mirroring runtime/ruby/units/wasi/resolve_path.rb). R0 is
# the errno (0 = ok), R1 the physical path on success.
#
# The parent is resolved physically via a `cd -P` subshell and the basename
# appended lexically, so `..`/symlinks in the parent are collapsed before the
# containment check — nesting can't launder an escape. A trailing "."/".."
# resolves the whole path as a directory instead. Final-component symlinks:
# a directory symlink is followed (via cd -P) when <follow> is 1; a file symlink
# cannot be followed (no readlink builtin) and returns ELOOP (32), stricter than
# Ruby (ADR-32). Check-then-open TOCTOU caveat carried over from ADR-14.
wasi_resolve_path() {
  local __p=$1 __dirfd=$2 __rel=$3 __follow=$4
  local -n __fds=${__p}wfds
  local -n __wpath=${__p}wpath
  if [[ ${__fds[$__dirfd]-} != 3 ]]; then
    R0=8 # EBADF: not a directory fd
    return 0
  fi
  local __root=${__wpath[$__dirfd]}
  local __joined=${__root%/}/$__rel
  local __base=${__joined##*/}
  local __real
  if [[ $__base == "." || $__base == ".." ]]; then
    # A "."/".." tail is never a symlink; resolve the whole path as a dir.
    __real=$(cd -P -- "$__joined" 2>/dev/null && pwd -P)
    if [[ -z $__real ]]; then
      R0=44 # ENOENT
      return 0
    fi
  else
    local __dir=${__joined%/*}
    local __realparent
    __realparent=$(cd -P -- "$__dir" 2>/dev/null && pwd -P)
    if [[ -z $__realparent ]]; then
      R0=44 # ENOENT: missing parent
      return 0
    fi
    __real=${__realparent%/}/$__base
    if [[ -h $__real ]]; then
      if (( __follow )); then
        local __target
        __target=$(cd -P -- "$__real" 2>/dev/null && pwd -P)
        if [[ -z $__target ]]; then
          R0=32 # ELOOP: file symlink can't be followed (no readlink builtin)
          return 0
        fi
        __real=$__target
      fi
      # follow=0: operate on the symlink itself (lstat shape); keep lexical path.
    fi
  fi
  if [[ $__real == "$__root" || $__real == "${__root%/}/"* ]]; then
    R1=$__real
    R0=0
    return 0
  fi
  R0=76 # ENOTCAPABLE: escapes the preopen root
  return 0
}
