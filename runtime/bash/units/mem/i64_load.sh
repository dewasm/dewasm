# requires: mem/check
# The top byte shifted by 56 wraps into the sign bit, which is exactly the
# signed-64 bit-pattern representation of i64 (ADR-11).
mem_i64_load() {
  local -n __m=${1}mem
  local a=$2
  mem_check "$1" "$a" 8 || return $?
  R0=$(( __m[a] | __m[a+1] << 8 | __m[a+2] << 16 | __m[a+3] << 24 \
       | __m[a+4] << 32 | __m[a+5] << 40 | __m[a+6] << 48 | __m[a+7] << 56 ))
  return 0
}
