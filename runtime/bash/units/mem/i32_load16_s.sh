# requires: mem/check
mem_i32_load16_s() {
  local -n __m=${1}mem
  local a=$2
  mem_check "$1" "$a" 2 || return $?
  R0=$(( (((__m[a] | __m[a+1] << 8) ^ 0x8000) - 0x8000) & 0xffffffff ))
  return 0
}
