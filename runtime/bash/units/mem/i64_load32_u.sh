# requires: mem/check
mem_i64_load32_u() {
  local -n __m=${1}mem
  local a=$2
  mem_check "$1" "$a" 4 || return $?
  R0=$(( __m[a] | __m[a+1] << 8 | __m[a+2] << 16 | __m[a+3] << 24 ))
  return 0
}
