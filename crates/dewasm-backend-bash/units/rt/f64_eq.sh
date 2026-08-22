rt_f64_eq() {
  local a=$1 b=$2
  if (( (a & 0x7fffffffffffffff) > 0x7ff0000000000000 || (b & 0x7fffffffffffffff) > 0x7ff0000000000000 )); then
    R0=0
    return 0
  fi
  R0=$(( a == b || ((a | b) & 0x7fffffffffffffff) == 0 ))
  return 0
}
