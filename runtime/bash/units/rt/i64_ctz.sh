# The arithmetic right shift only drags sign bits in above the lowest set
# bit, so the count is still exact for negative patterns.
rt_i64_ctz() {
  local x=$1 n=0
  if (( x == 0 )); then
    R0=64
    return 0
  fi
  while (( (x & 1) == 0 )); do
    (( n += 1, x >>= 1 ))
  done
  R0=$n
  return 0
}
