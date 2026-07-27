# requires: wasi/read_path, wasi/resolve_path, wasi/filetype, wasi/pack_filestat, wasi/file_size
# WASI path_filestat_get (ADR-32): lookupflags bit 0 = SYMLINK_FOLLOW, passed
# straight through to wasi_resolve_path's <follow> so a not-followed final
# symlink is stat'd as itself (filetype 7), mirroring
# runtime/ruby/units/wasi/path_filestat_get.rb's stat/lstat split. Filetype
# comes from the test builtins (wasi_filetype). Size is only meaningful for a
# regular file: for that case, an open file fd on the *same* resolved host
# path wins over the on-disk size (last one found, matching D1's own
# last-flush-wins rule for two fds on one file) so a write-then-stat on the
# same path is coherent before the buffer is flushed; otherwise the on-disk
# size (wasi_file_size) is used. Every other filetype (directory, symlink,
# device, socket, fifo) reports size 0 — Bash cannot introspect a symlink's
# target-string length or a device's size, and a directory's size isn't
# tracked, so this is a documented approximation, not a real stat().
wasi_path_filestat_get() {
  local __p=$1 __dirfd=$2 __flags=$3 __path_ptr=$4 __path_len=$5 __buf=$6
  wasi_read_path "$__p" "$__path_ptr" "$__path_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __rel=$R1
  wasi_resolve_path "$__p" "$__dirfd" "$__rel" "$(( __flags & 1 ))" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __host=$R1
  wasi_filetype "$__host" || return $?
  local __ft=$R1
  local __size=0
  if [[ $__ft == 4 ]]; then
    local -n __fds=${__p}wfds
    local -n __wpath=${__p}wpath
    local __i __match=-1
    for __i in "${!__fds[@]}"; do
      if [[ ${__fds[$__i]} == 2 && ${__wpath[$__i]-} == "$__host" ]]; then
        __match=$__i
      fi
    done
    if (( __match >= 0 )); then
      local -n __wbuf=${__p}wbuf${__match}
      __size=${#__wbuf[@]}
    else
      wasi_file_size "$__host" || return $?
      __size=$R1
    fi
  fi
  wasi_pack_filestat "$__p" "$__buf" "$__ft" "$__size" || return $?
  R0=0
  return 0
}
