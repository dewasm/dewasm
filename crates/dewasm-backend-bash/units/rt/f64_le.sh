# See rt/f64_lt for the order key.
rt_f64_le() {
  local a=$1 b=$2
  if (( (a & 0x7fffffffffffffff) > 0x7ff0000000000000 || (b & 0x7fffffffffffffff) > 0x7ff0000000000000 )); then
    R0=0
    return 0
  fi
  R0=$(( (a < 0 ? -(a & 0x7fffffffffffffff) : a) <= (b < 0 ? -(b & 0x7fffffffffffffff) : b) ))
  return 0
}
