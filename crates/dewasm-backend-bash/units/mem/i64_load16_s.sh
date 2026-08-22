# requires: mem/check
mem_i64_load16_s() {
  local -n __m=${1}mem
  local a=$2
  mem_check "$1" "$a" 2 || return $?
  local a1=$(( a + 1 ))
  R0=$(( ((__m[$a] | __m[$a1] << 8) ^ 0x8000) - 0x8000 ))
  return 0
}
