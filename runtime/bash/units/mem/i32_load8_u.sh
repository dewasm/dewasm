# requires: mem/check
mem_i32_load8_u() {
  local -n __m=${1}mem
  local a=$2
  mem_check "$1" "$a" 1 || return $?
  R0=$(( __m[a] ))
  return 0
}
