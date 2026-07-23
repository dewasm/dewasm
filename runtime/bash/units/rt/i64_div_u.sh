# requires: rt/trap
# Unsigned 64-bit division over signed-64 bit patterns (Hacker's Delight):
# a negative pattern means the unsigned value has the top bit set.
rt_i64_div_u() {
  local a=$1 b=$2 q r
  if (( b == 0 )); then rt_trap 'integer divide by zero'; return $?; fi
  if (( b < 0 )); then
    # divisor >= 2^63: quotient is 0 or 1 (unsigned compare via sign flip)
    if (( (a ^ 1 << 63) >= (b ^ 1 << 63) )); then R0=1; else R0=0; fi
    return 0
  fi
  if (( a >= 0 )); then
    R0=$(( a / b ))
    return 0
  fi
  (( q = ((a >> 1) & 0x7fffffffffffffff) / b << 1, r = a - q * b ))
  if (( r < 0 || r >= b )); then (( q += 1 )); fi
  R0=$q
  return 0
}
