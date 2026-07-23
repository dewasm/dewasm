# requires: mem/check
# /dev/urandom through the read builtin: no external commands (ADR-5) and
# no SRANDOM (bash 5.1+; the floor is 5.0). A NUL byte reads as ''.
wasi_random_get() {
  local __p=$1 __ptr=$2 __len=$3
  local -n __m=${__p}mem
  mem_check "$__p" "$__ptr" "$__len" || return $?
  local LC_ALL=C
  local __i __ch __b
  for (( __i = 0; __i < __len; __i++ )); do
    IFS= read -r -d '' -n 1 __ch || __ch=''
    if [[ -z $__ch ]]; then
      __b=0
    else
      printf -v __b '%d' "'$__ch"
    fi
    __m[__ptr + __i]=$(( __b & 0xff ))
  done < /dev/urandom
  R0=0
  return 0
}
