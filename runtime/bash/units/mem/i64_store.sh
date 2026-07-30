# requires: mem/check
# The arithmetic right shift on negative patterns is harmless: & 0xff
# masks the dragged-in sign bits per byte.
mem_i64_store() {
  local -n __m=${1}mem
  local a=$2 v=$3
  mem_check "$1" "$a" 8 || return $?
  local a1=$(( a + 1 )) a2=$(( a + 2 )) a3=$(( a + 3 )) a4=$(( a + 4 ))
  local a5=$(( a + 5 )) a6=$(( a + 6 )) a7=$(( a + 7 ))
  __m[$a]=$(( v & 0xff ))
  __m[$a1]=$(( v >> 8 & 0xff ))
  __m[$a2]=$(( v >> 16 & 0xff ))
  __m[$a3]=$(( v >> 24 & 0xff ))
  __m[$a4]=$(( v >> 32 & 0xff ))
  __m[$a5]=$(( v >> 40 & 0xff ))
  __m[$a6]=$(( v >> 48 & 0xff ))
  __m[$a7]=$(( v >> 56 & 0xff ))
  return 0
}
