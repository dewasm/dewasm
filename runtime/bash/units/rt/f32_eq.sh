rt_f32_eq() {
  local a=$1 b=$2
  if (( (a & 0x7fffffff) > 0x7f800000 || (b & 0x7fffffff) > 0x7f800000 )); then
    R0=0
    return 0
  fi
  R0=$(( a == b || ((a | b) & 0x7fffffff) == 0 ))
  return 0
}
