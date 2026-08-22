# See rt/f32_lt for the order key.
rt_f32_le() {
  local a=$1 b=$2
  if (( (a & 0x7fffffff) > 0x7f800000 || (b & 0x7fffffff) > 0x7f800000 )); then
    R0=0
    return 0
  fi
  R0=$(( (a < 0x80000000 ? a : -(a & 0x7fffffff)) <= (b < 0x80000000 ? b : -(b & 0x7fffffff)) ))
  return 0
}
