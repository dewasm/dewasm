# requires: mem/check, mem/i32_store
# The args_get/environ_get ABI: a table of 4-byte LE pointers at list_ptr,
# NUL-terminated strings packed contiguously from buf_ptr. $2 is the name of the array holding the strings.
wasi_write_string_list() {
  local __p=$1 __list_ptr=$3 __buf_ptr=$4
  local -n __m=${__p}mem
  local -n __strs=$2
  local LC_ALL=C
  local __i __j __s __n __b __mk
  for (( __i = 0; __i < ${#__strs[@]}; __i++ )); do
    __s=${__strs[__i]}
    __n=${#__s}
    mem_i32_store "$__p" $(( __list_ptr + __i * 4 )) "$__buf_ptr" || return $?
    mem_check "$__p" "$__buf_ptr" $(( __n + 1 )) || return $?
    for (( __j = 0; __j < __n; __j++ )); do
      printf -v __b '%d' "'${__s:__j:1}"
      __mk=$(( __buf_ptr + __j ))
      __m[$__mk]=$(( __b & 0xff ))
    done
    __mk=$(( __buf_ptr + __n ))
    __m[$__mk]=0
    (( __buf_ptr += __n + 1 ))
  done
  R0=0
  return 0
}
