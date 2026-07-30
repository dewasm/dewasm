# requires: mem/check
mem_i64_load32_s() {
  local -n __m=${1}mem
  local a=$2
  mem_check "$1" "$a" 4 || return $?
  local a1=$(( a + 1 )) a2=$(( a + 2 )) a3=$(( a + 3 ))
  R0=$(( ((__m[$a] | __m[$a1] << 8 | __m[$a2] << 16 | __m[$a3] << 24) ^ 0x80000000) - 0x80000000 ))
  return 0
}
