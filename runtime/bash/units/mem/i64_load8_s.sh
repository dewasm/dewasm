# requires: mem/check
mem_i64_load8_s() {
  local -n __m=${1}mem
  local a=$2
  mem_check "$1" "$a" 1 || return $?
  R0=$(( (__m[a] ^ 0x80) - 0x80 ))
  return 0
}
