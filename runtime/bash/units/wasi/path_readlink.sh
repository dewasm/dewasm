# requires: mem/check, mem/i32_store, wasi/read_path, wasi/resolve_path
# WASI path_readlink (ADR-40): read the target string of the symlink named by
# <path> under the dirfd into the guest buffer, returning the number of bytes
# written (truncated to buf_len, never NUL-terminated). Resolution is NOFOLLOW
# (the link itself is the target of the call), so only the link's parent is
# sandbox-contained; reading the link's own target uses a single `--`-guarded
# `readlink` — a licensed external command (beyond ADR-34 D2's four) used only
# for this syscall, distinct from path *resolution*, which still cannot follow a
# file symlink in pure bash (D3). A missing path is ENOENT (44); a non-symlink
# is EINVAL (28).
wasi_path_readlink() {
  local __p=$1 __dirfd=$2 __path_ptr=$3 __path_len=$4 __buf_ptr=$5 __buf_len=$6 __bufused_ptr=$7
  wasi_read_path "$__p" "$__path_ptr" "$__path_len" || return $?
  if (( R0 != 0 )); then return 0; fi
  local __rel=$R1
  wasi_resolve_path "$__p" "$__dirfd" "$__rel" 0 || return $?
  if (( R0 != 0 )); then return 0; fi
  local __link=$R1
  if [[ ! -h $__link ]]; then
    if [[ -e $__link ]]; then
      R0=28 # EINVAL: not a symbolic link
    else
      R0=44 # ENOENT
    fi
    return 0
  fi
  local __target
  __target=$(readlink -- "$__link")
  local LC_ALL=C
  local -n __m=${__p}mem
  local __tlen=${#__target} __n
  __n=$__tlen
  if (( __n > __buf_len )); then __n=$__buf_len; fi
  mem_check "$__p" "$__buf_ptr" "$__n" || return $?
  local __i __b __mk
  for (( __i = 0; __i < __n; __i++ )); do
    printf -v __b '%d' "'${__target:__i:1}"
    __mk=$(( __buf_ptr + __i ))
    __m[$__mk]=$(( __b & 0xff ))
  done
  mem_i32_store "$__p" "$__bufused_ptr" "$__n" || return $?
  R0=0
  return 0
}
