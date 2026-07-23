# requires: mem/i32_store
wasi_environ_sizes_get() {
  local __p=$1 __count_ptr=$2 __size_ptr=$3
  local -n __strs=${__p}wenv
  local LC_ALL=C
  local __n=0 __s
  for __s in "${__strs[@]}"; do
    (( __n += ${#__s} + 1 ))
  done
  mem_i32_store "$__p" "$__count_ptr" "${#__strs[@]}" || return $?
  mem_i32_store "$__p" "$__size_ptr" "$__n" || return $?
  R0=0
  return 0
}
