# requires: mem/check
# The arithmetic right shift on negative patterns is harmless: & 0xff
# masks the dragged-in sign bits per byte.
mem_i64_store() {
  local -n __m=${1}mem
  local a=$2 v=$3
  mem_check "$1" "$a" 8 || return $?
  __m[a]=$(( v & 0xff ))
  __m[a+1]=$(( v >> 8 & 0xff ))
  __m[a+2]=$(( v >> 16 & 0xff ))
  __m[a+3]=$(( v >> 24 & 0xff ))
  __m[a+4]=$(( v >> 32 & 0xff ))
  __m[a+5]=$(( v >> 40 & 0xff ))
  __m[a+6]=$(( v >> 48 & 0xff ))
  __m[a+7]=$(( v >> 56 & 0xff ))
  return 0
}
