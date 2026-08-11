# requires: rt/f64_round_pack
# Chunked long division, 9 bits per step: the running remainder stays below 2^53, so r<<9 < 2^62 never overflows; 6 steps give a 55-bit quotient with the final remainder as the sticky.
# Division only ever sees non-negative operands (bash / truncates toward zero).
rt_f64_div() {
  local a=$1 b=$2 pa pb s ea eb ma mb q r i
  (( pa = a & 0x7fffffffffffffff, pb = b & 0x7fffffffffffffff ))
  (( s = ((a ^ b) >> 63) & 1 ))
  if (( pa > 0x7ff0000000000000 || pb > 0x7ff0000000000000 )); then
    R0=$(( 0x7ff8000000000000 ))
    return 0
  fi
  if (( pa == 0x7ff0000000000000 || pb == 0x7ff0000000000000 )); then
    if (( pa == pb )); then
      R0=$(( 0x7ff8000000000000 ))
    elif (( pa == 0x7ff0000000000000 )); then
      R0=$(( (s << 63) | 0x7ff0000000000000 ))
    else
      R0=$(( s << 63 ))
    fi
    return 0
  fi
  if (( pa == 0 && pb == 0 )); then
    R0=$(( 0x7ff8000000000000 ))
    return 0
  fi
  if (( pb == 0 )); then
    R0=$(( (s << 63) | 0x7ff0000000000000 ))
    return 0
  fi
  if (( pa == 0 )); then
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
  (( q = 0, r = ma ))
  for (( i = 0; i < 6; i++ )); do
    (( q = (q << 9) | ((r << 9) / mb), r = (r << 9) % mb ))
  done
  rt_f64_round_pack "$s" $(( ea - eb - 1 )) "$q" $(( r != 0 ))
  return 0
}
