# `wasi_resolve_path <p> <dirfd> <rel> <follow>`
# Resolves a guest-relative path against a directory `fd` to a physical host path.
# The result is confined to that `dirfd`'s own stored root.
# This mirrors `crates/dewasm-backend-ruby/units/wasi/resolve_path.rb`.
# R0 is the `errno` (0 = ok), R1 the physical path on success.
#
# The parent is resolved physically via a `cd -P` subshell.
# The last component is appended lexically.
# So `..`/symbolic links in the parent are collapsed before the containment check.
# Nesting then can't launder an escape.
# A trailing "."/".." resolves the whole path as a directory instead.
# Final-component symbolic links:
# - a directory symbolic link is followed (via `cd -P`) when `<follow>` is 1;
# - a file symbolic link cannot be followed (no `readlink` builtin) and returns ELOOP (32).
#   That is stricter than Ruby.
# As in Ruby, check-then-open is a TOCTOU risk: a resolved path can be swapped before it is opened.
# So this is a single-process research/example runtime, not a multi-tenant sandbox host.
#
# A leading "/" makes the path absolute, which escapes the `dirfd` sandbox before any join.
# That is NOTCAPABLE (76), not a lexical join under the root.
# A `dirfd` that names an open non-directory `fd` is ENOTDIR (54); an unopened `fd` is EBADF (8).
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
  # Strip a trailing slash for resolution, remember it, and re-check the directory constraint below.
  # The parent/last-component split below would otherwise read an empty last component.
  # It would then misresolve every slash-suffixed name (issue #42).
  local __slash=0
  if [[ $__rel == */ ]]; then
    __slash=1
    while [[ $__rel == */ ]]; do __rel=${__rel%/}; done
  fi
  # Reject a path that lexically goes above the `dirfd` root via `..`.
  # It is an escape even when the resulting physical parent does not exist.
  # So the post-resolution containment check cannot catch it, and would misreport it as ENOENT.
  # A pure component walk: each name is +1 depth, and `..` is -1.
  # A depth that ever goes negative has escaped the root.
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
    # A "."/".." tail is never a symbolic link; resolve the whole path as a directory.
    __real=$(cd -P -- "$__joined" 2>/dev/null && pwd -P)
    if [[ -z $__real ]]; then
      # `cd -P` cannot enter a non-directory, and a preopen root need not be one.
      # `wasi-libc` addresses a path that *is* a preopen as that preopen's `fd` plus the path ".".
      # An example is the `zeroperl` reactor's "/dev/null", which lands here.
      # Collapse that "." onto the root, which is already stored physically.
      # That is the same resolution Ruby's `File.realpath` and Perl's `Cwd::realpath` give a file.
      # Only the root is collapsed: a "somefile/." deeper in the tree keeps failing.
      if [[ $__base == "." && ${__joined%/.} == "${__root%/}" && -e $__root ]]; then
        __real=$__root
      else
        R0=44 # ENOENT
        return 0
      fi
    fi
  else
    local __dir=${__joined%/*}
    # A root-preopen entry ("/etc") strips to an empty parent, not the file system root.
    # `cd -P -- ""` is a Bash error ("null directory"), not a no-op.
    # Without this, every path under a root preopen (`WASI_DIRS=('/::/')`) would resolve to ENOENT.
    # That happens regardless of containment.
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
      # `follow=0`: operate on the symbolic link itself (`lstat` shape); keep lexical path.
    fi
  fi
  if [[ $__real == "$__root" || $__real == "${__root%/}/"* ]]; then
    # Slash-suffixed names may only resolve to a directory.
    # `-e` follows symbolic links as the slash requires.
    # A missing target is each caller's case.
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
