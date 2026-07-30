# requires: mem/check
mem_i64_store8() {
  local -n __m=${1}mem
  local a=$2
  mem_check "$1" "$a" 1 || return $?
  __m[$a]=$(( $3 & 0xff ))
  return 0
}
