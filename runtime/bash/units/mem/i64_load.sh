# requires: mem/check
# The top byte shifted by 56 wraps into the sign bit, which is exactly the
# signed-64 bit-pattern representation of i64.
mem_i64_load() {
  local -n __m=${1}mem
  local a=$2
  mem_check "$1" "$a" 8 || return $?
  local a1=$(( a + 1 )) a2=$(( a + 2 )) a3=$(( a + 3 )) a4=$(( a + 4 ))
  local a5=$(( a + 5 )) a6=$(( a + 6 )) a7=$(( a + 7 ))
  R0=$(( __m[$a] | __m[$a1] << 8 | __m[$a2] << 16 | __m[$a3] << 24 \
       | __m[$a4] << 32 | __m[$a5] << 40 | __m[$a6] << 48 | __m[$a7] << 56 ))
  return 0
}
