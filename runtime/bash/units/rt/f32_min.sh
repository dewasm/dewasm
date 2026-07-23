# wasm min on u32 patterns; see rt/f64_min.
rt_f32_min() {
  local a=$1 b=$2 ka kb
  if (( (a & 0x7fffffff) > 0x7f800000 || (b & 0x7fffffff) > 0x7f800000 )); then
    R0=$(( 0x7fc00000 ))
    return 0
  fi
  (( ka = a < 0x80000000 ? a : -(a & 0x7fffffff) ))
  (( kb = b < 0x80000000 ? b : -(b & 0x7fffffff) ))
  if (( ka < kb )); then
    R0=$(( a ))
  elif (( kb < ka )); then
    R0=$(( b ))
  else
    R0=$(( a | b ))
  fi
  return 0
}
