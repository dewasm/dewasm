# requires: rt/f64_round_pack
# Restoring digit-by-digit square root, 55 iterations, no 128-bit math:
# the radicand's low half is all zeros, so feeding 2 bits per step keeps the running remainder below 2^59.
# The 55-bit root plus remainder-sticky through round_pack gives correct RNE (sqrt never lands on a midpoint).
rt_f64_sqrt() {
  local a=$1 pa ea ma h hi r rem k b2 t
  (( pa = a & 0x7fffffffffffffff ))
  if (( pa > 0x7ff0000000000000 )); then
    R0=$(( 0x7ff8000000000000 ))
    return 0
  fi
  if (( pa == 0 )); then
    R0=$(( a ))
    return 0
  fi
  if (( a < 0 )); then
    R0=$(( 0x7ff8000000000000 ))
    return 0
  fi
  if (( pa == 0x7ff0000000000000 )); then
    R0=$(( a ))
    return 0
  fi
  if (( pa >> 52 )); then
    (( ma = (pa & 0xfffffffffffff) | 1 << 52, ea = (pa >> 52) - 1023 ))
  else
    (( ma = pa, ea = -1022 ))
  fi
  while (( ma < 1 << 52 )); do (( ma <<= 1, ea -= 1 )); done
  if (( (ea - 52) % 2 )); then
    (( ma <<= 1, h = (ea - 53) / 2 ))
  else
    (( h = (ea - 52) / 2 ))
  fi
  (( hi = ma << 2, r = 0, rem = 0 ))
  for (( k = 27; k >= 0; k-- )); do
    (( b2 = (hi >> (2 * k)) & 3, rem = (rem << 2) | b2, t = (r << 2) | 1 ))
    if (( rem >= t )); then
      (( rem -= t, r = (r << 1) | 1 ))
    else
      (( r <<= 1 ))
    fi
  done
  for (( k = 26; k >= 0; k-- )); do
    (( rem <<= 2, t = (r << 2) | 1 ))
    if (( rem >= t )); then
      (( rem -= t, r = (r << 1) | 1 ))
    else
      (( r <<= 1 ))
    fi
  done
  rt_f64_round_pack 0 $(( h + 25 )) "$r" $(( rem != 0 ))
  return 0
}
