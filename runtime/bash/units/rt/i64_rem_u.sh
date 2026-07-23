# requires: rt/trap
# See rt/i64_div_u for the unsigned-over-signed-patterns strategy.
rt_i64_rem_u() {
  local a=$1 b=$2 q r
  if (( b == 0 )); then rt_trap 'integer divide by zero'; return $?; fi
  if (( b < 0 )); then
    if (( (a ^ 1 << 63) >= (b ^ 1 << 63) )); then R0=$(( a - b )); else R0=$a; fi
    return 0
  fi
  if (( a >= 0 )); then
    R0=$(( a % b ))
    return 0
  fi
  (( q = ((a >> 1) & 0x7fffffffffffffff) / b << 1, r = a - q * b ))
  if (( r < 0 || r >= b )); then (( r -= b )); fi
  R0=$r
  return 0
}
