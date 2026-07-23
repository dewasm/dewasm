# requires: rt/f64_round_pack
# The 106-bit mantissa product uses a 26-bit split: every partial stays
# below 2^55 (a 32-bit split would wrap at ~2^64). Operands are
# pre-normalized to [2^52, 2^53) so P >= 2^104 and m = P>>51 >= 2^53,
# keeping round_pack in its right-normalizing regime.
rt_f64_mul() {
  local a=$1 b=$2 pa pb s ea eb ma mb a0 a1 b0 b1 p0 mid hi lo m sk
  (( pa = a & 0x7fffffffffffffff, pb = b & 0x7fffffffffffffff ))
  (( s = ((a ^ b) >> 63) & 1 ))
  if (( pa > 0x7ff0000000000000 || pb > 0x7ff0000000000000 )); then
    R0=$(( 0x7ff8000000000000 ))
    return 0
  fi
  if (( pa == 0x7ff0000000000000 || pb == 0x7ff0000000000000 )); then
    if (( pa == 0 || pb == 0 )); then
      R0=$(( 0x7ff8000000000000 ))
    else
      R0=$(( (s << 63) | 0x7ff0000000000000 ))
    fi
    return 0
  fi
  if (( pa == 0 || pb == 0 )); then
    R0=$(( s << 63 ))
    return 0
  fi
  if (( pa >> 52 )); then
    (( ma = (pa & 0xfffffffffffff) | 1 << 52, ea = (pa >> 52) - 1023 ))
  else
    (( ma = pa, ea = -1022 ))
  fi
  if (( pb >> 52 )); then
    (( mb = (pb & 0xfffffffffffff) | 1 << 52, eb = (pb >> 52) - 1023 ))
  else
    (( mb = pb, eb = -1022 ))
  fi
  while (( ma < 1 << 52 )); do (( ma <<= 1, ea -= 1 )); done
  while (( mb < 1 << 52 )); do (( mb <<= 1, eb -= 1 )); done
  (( a1 = ma >> 26, a0 = ma & 0x3ffffff, b1 = mb >> 26, b0 = mb & 0x3ffffff ))
  (( p0 = a0 * b0 ))
  (( mid = a1 * b0 + a0 * b1 + (p0 >> 26) ))
  (( hi = a1 * b1 + (mid >> 26) ))
  (( lo = ((mid & 0x3ffffff) << 26) | (p0 & 0x3ffffff) ))
  (( m = (hi << 1) | ((lo >> 51) & 1), sk = (lo & ((1 << 51) - 1)) != 0 ))
  rt_f64_round_pack "$s" $(( ea + eb )) "$m" "$sk"
  return 0
}
