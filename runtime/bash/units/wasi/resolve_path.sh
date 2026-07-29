# wasi_resolve_path <p> <dirfd> <rel> <follow>: resolve a guest-relative path
# against a directory fd to a physical host path, confined to that dirfd's own
# stored root (ADR-34, mirroring runtime/ruby/units/wasi/resolve_path.rb). R0 is
# the errno (0 = ok), R1 the physical path on success.
#
# The parent is resolved physically via a `cd -P` subshell and the basename
# appended lexically, so `..`/symlinks in the parent are collapsed before the
# containment check — nesting can't launder an escape. A trailing "."/".."
# resolves the whole path as a directory instead. Final-component symlinks:
# a directory symlink is followed (via cd -P) when <follow> is 1; a file symlink
# cannot be followed (no readlink builtin) and returns ELOOP (32), stricter than
# Ruby (ADR-34). Check-then-open TOCTOU caveat carried over from ADR-14.
#
# A leading "/" makes the path absolute, which escapes the dirfd sandbox before
# any join — NOTCAPABLE (76), not a lexical join under the root (ADR-40). A
# dirfd that names an open non-directory fd is ENOTDIR (54); an unopened fd is
# EBADF (8).
wasi_resolve_path() {
  local __p=$1 __dirfd=$2 __rel=$3 __follow=$4
  local -n __fds=${__p}wfds
  local -n __wpath=${__p}wpath
  local __kind=${__fds[$__dirfd]-}
  if [[ -z $__kind ]]; then
    R0=8 # EBADF: unopened fd
    return 0
  fi
  if [[ $__kind != 3 ]]; then
    R0=54 # ENOTDIR: dirfd is an open file/stdio fd, not a directory
    return 0
  fi
  if [[ $__rel == /* ]]; then
    R0=76 # ENOTCAPABLE: an absolute path escapes the preopen sandbox
    return 0
  fi
  # A trailing slash constrains the final component to a directory (POSIX
  # pathname resolution; issue #42). Strip it for resolution — the
  # parent-plus-basename split below would otherwise read an empty basename
  # and resolve the *whole* path as a directory (ENOENT for every
  # slash-suffixed name over a non-directory, and even for "sub/" on mkdir) —
  # and re-check the constraint against the resolved target before returning.
  local __slash=0
  if [[ $__rel == */ ]]; then
    __slash=1
    while [[ $__rel == */ ]]; do __rel=${__rel%/}; done
  fi
  # Reject a path that lexically ascends above the dirfd root via `..` — an
  # escape even when the resulting physical parent does not exist (so it cannot
  # be caught by the post-resolution containment check, which would misreport it
  # as ENOENT). A pure component walk: each name is +1 depth, `..` is -1, and a
  # depth that ever goes negative has escaped the root (ADR-40).
  local __walk=$__rel __comp __depth=0
  while [[ -n $__walk ]]; do
    __comp=${__walk%%/*}
    if [[ $__walk == */* ]]; then __walk=${__walk#*/}; else __walk=''; fi
    case $__comp in
      '' | .) : ;;
      ..)
        (( __depth-- , 1 ))
        if (( __depth < 0 )); then R0=76; return 0; fi
        ;;
      *) (( __depth++ , 1 )) ;;
    esac
  done
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
    # A root-preopen entry ("/etc") strips to an empty parent, not the
    # filesystem root — `cd -P -- ""` is a bash error ("null directory"), not
    # a no-op. Without this, every path under a root preopen (`WASI_DIRS=('/::/')`)
    # would resolve to ENOENT regardless of containment.
    [[ -z $__dir ]] && __dir=/
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
    # The trailing-slash directory gate: `-e` follows symlinks, exactly as the
    # slash forces, so "link-to-file/" is ENOTDIR while "link-to-dir/" passes.
    # A missing target passes too: which errno (or success, for a create) that
    # becomes is each caller's own business.
    if (( __slash )) && [[ -e $__real && ! -d $__real ]]; then
      R0=54 # ENOTDIR: a slash-suffixed name resolved to a non-directory
      return 0
    fi
    R1=$__real
    R0=0
    return 0
  fi
  R0=76 # ENOTCAPABLE: escapes the preopen root
  return 0
}
