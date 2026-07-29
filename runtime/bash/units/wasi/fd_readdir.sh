# requires: mem/check, mem/i32_store, wasi/filetype
# WASI fd_readdir (ADR-34): the first call for a dir fd snapshots the listing
# into <p>wdn<fd>/<p>wdt<fd> (names/filetypes) — glob the host directory once
# with dotglob+nullglob (restored unconditionally right after, via the saved
# `shopt -p` string), strip to basenames, insertion-sort byte-wise
# (`LC_ALL=C`; Bash has no builtin sort), then prepend "."/".." (type 3).
# Later calls for the same fd reuse the cached snapshot, so the cookie is a
# stable 1-based index into a point-in-time listing, not a live cursor under
# concurrent mutation — mirrors runtime/ruby/units/wasi/fd_readdir.rb's
# `WasiDir#entries` cache and Ruby's own `.sort` (byte order under LC_ALL=C).
# Packs the 24-byte dirent (d_next u64 resume cookie, d_ino u64 = 0, d_namlen
# u32, d_type u8 + 3 pad) followed by the unpadded name bytes; a dirent may be
# legally truncated once buf_len runs out (bufused == buf_len signals more
# entries remain), same contract as Ruby's byteslice truncation.
wasi_fd_readdir() {
  local __p=$1 __fd=$2 __buf_ptr=$3 __buf_len=$4 __cookie=$5 __bufused_ptr=$6
  local -n __fds=${__p}wfds
  if [[ ${__fds[$__fd]-} != 3 ]]; then
    R0=8 # EBADF
    return 0
  fi
  local -n __wrbase=${__p}wrbase
  if (( (__wrbase[$__fd] & 0x4000) == 0 )); then
    R0=76 # ENOTCAPABLE: fd lacks FD_READDIR
    return 0
  fi
  if ! declare -p "${__p}wdn${__fd}" &>/dev/null; then
    local -n __wpath=${__p}wpath
    local __root=${__wpath[$__fd]}
    local __save
    __save=$(shopt -p dotglob nullglob)
    shopt -s dotglob nullglob
    local __paths=("$__root"/*)
    eval "$__save"
    local LC_ALL=C
    local __gi __raw=()
    for __gi in "${__paths[@]}"; do
      __raw+=("${__gi##*/}")
    done
    local __rn=${#__raw[@]} __rj __rkey
    for (( __gi = 1; __gi < __rn; __gi++ )); do
      __rkey=${__raw[__gi]}
      __rj=$(( __gi - 1 ))
      while (( __rj >= 0 )) && [[ ${__raw[__rj]} > $__rkey ]]; do
        __raw[__rj + 1]=${__raw[__rj]}
        (( __rj-- ))
      done
      __raw[__rj + 1]=$__rkey
    done
    declare -ga "${__p}wdn${__fd}=(. ..)"
    declare -ga "${__p}wdt${__fd}=(3 3)"
    local -n __namesref=${__p}wdn${__fd}
    local -n __typesref=${__p}wdt${__fd}
    for __gi in "${__raw[@]}"; do
      wasi_filetype "$__root/$__gi" || return $?
      __namesref+=("$__gi")
      __typesref+=("$R1")
    done
  fi
  local -n __names=${__p}wdn${__fd}
  local -n __types=${__p}wdt${__fd}
  local __total=${#__names[@]}
  local -n __m=${__p}mem
  local LC_ALL=C
  local __out=() __outlen=0 __ri=$__cookie __name __nlen __next __ck __b
  # The u64 cookie arrives as bash's signed-64 bit pattern (ADR-11): a
  # negative value is a huge unsigned position past any snapshot's end, so
  # report end-of-directory (an empty result, matching wasmtime) instead of
  # letting the loop read negative subscripts.
  if (( __ri < 0 )); then __ri=$__total; fi
  while (( __ri < __total && __outlen < __buf_len )); do
    __name=${__names[__ri]}
    __nlen=${#__name}
    __next=$(( __ri + 1 ))
    __out[__outlen++]=$(( __next & 0xff ))
    __out[__outlen++]=$(( __next >> 8 & 0xff ))
    __out[__outlen++]=$(( __next >> 16 & 0xff ))
    __out[__outlen++]=$(( __next >> 24 & 0xff ))
    __out[__outlen++]=$(( __next >> 32 & 0xff ))
    __out[__outlen++]=$(( __next >> 40 & 0xff ))
    __out[__outlen++]=$(( __next >> 48 & 0xff ))
    __out[__outlen++]=$(( __next >> 56 & 0xff ))
    __out[__outlen++]=0 # d_ino (8 bytes, always 0)
    __out[__outlen++]=0
    __out[__outlen++]=0
    __out[__outlen++]=0
    __out[__outlen++]=0
    __out[__outlen++]=0
    __out[__outlen++]=0
    __out[__outlen++]=0
    __out[__outlen++]=$(( __nlen & 0xff ))
    __out[__outlen++]=$(( __nlen >> 8 & 0xff ))
    __out[__outlen++]=$(( __nlen >> 16 & 0xff ))
    __out[__outlen++]=$(( __nlen >> 24 & 0xff ))
    __out[__outlen++]=${__types[__ri]}
    __out[__outlen++]=0 # 3 pad bytes
    __out[__outlen++]=0
    __out[__outlen++]=0
    for (( __ck = 0; __ck < __nlen; __ck++ )); do
      printf -v __b '%d' "'${__name:__ck:1}"
      __out[__outlen++]=$(( __b & 0xff ))
    done
    (( __ri++ ))
  done
  if (( __outlen > __buf_len )); then __outlen=$__buf_len; fi
  mem_check "$__p" "$__buf_ptr" "$__outlen" || return $?
  for (( __ck = 0; __ck < __outlen; __ck++ )); do
    __m[__buf_ptr + __ck]=${__out[__ck]}
  done
  mem_i32_store "$__p" "$__bufused_ptr" "$__outlen" || return $?
  R0=0
  return 0
}
