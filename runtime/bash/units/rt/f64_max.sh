# wasm max: NaN operand -> canonical NaN; max(-0,+0) = +0 via a&b.
rt_f64_max() {
  local a=$1 b=$2 ka kb
  if (( (a & 0x7fffffffffffffff) > 0x7ff0000000000000 || (b & 0x7fffffffffffffff) > 0x7ff0000000000000 )); then
    R0=$(( 0x7ff8000000000000 ))
    return 0
  fi
  (( ka = a < 0 ? -(a & 0x7fffffffffffffff) : a ))
  (( kb = b < 0 ? -(b & 0x7fffffffffffffff) : b ))
  if (( ka > kb )); then
    R0=$(( a ))
  elif (( kb > ka )); then
    R0=$(( b ))
  else
    R0=$(( a & b ))
  fi
  return 0
}
