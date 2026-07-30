# requires: mem/check, rt/trap
# mem_init <prefix> <data-array-name> <dst> <src> <len>; a dropped data
# segment is an empty array, so its length check traps like the spec asks.
# The source <data-array-name> is an indexed byte array (arithmetic subscript
# ok); only the assoc destination needs a pre-expanded key.
mem_init() {
  local -n __m=${1}mem
  local -n __d=$2
  local d=$3 s=$4 n=$5 i k
  if (( s + n > ${#__d[@]} )); then
    rt_trap 'out of bounds memory access'
    return $?
  fi
  mem_check "$1" "$d" "$n" || return $?
  for (( i = 0; i < n; i++ )); do
    k=$(( d + i ))
    __m[$k]=$(( __d[s+i] ))
  done
  return 0
}
