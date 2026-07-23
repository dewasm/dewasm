# requires: mem/check
mem_i64_store32() {
  local -n __m=${1}mem
  local a=$2 v=$3
  mem_check "$1" "$a" 4 || return $?
  __m[a]=$(( v & 0xff ))
  __m[a+1]=$(( v >> 8 & 0xff ))
  __m[a+2]=$(( v >> 16 & 0xff ))
  __m[a+3]=$(( v >> 24 & 0xff ))
  return 0
}
