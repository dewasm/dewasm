# requires: mem/check
mem_i32_store() {
  local -n __m=${1}mem
  local a=$2 v=$3
  mem_check "$1" "$a" 4 || return $?
  local a1=$(( a + 1 )) a2=$(( a + 2 )) a3=$(( a + 3 ))
  __m[$a]=$(( v & 0xff ))
  __m[$a1]=$(( v >> 8 & 0xff ))
  __m[$a2]=$(( v >> 16 & 0xff ))
  __m[$a3]=$(( v >> 24 & 0xff ))
  return 0
}
