# The f64 round-and-pack core: R0 = (-1)^s * m * 2^(e-53),
# rounded to nearest-even, where sk means "strictly more than m, by less
# than one unit in m's last place".
#
# Contract: 0 <= m < 2^63, and if m < 2^53 then sk == 0. Callers must
# never require a left normalization with sticky bits pending (add is
# exact in its cancellation cases; mul/div/sqrt pre-normalize operands).
# The subnormal shift d is clamped to 54: it can reach ~2100 for tiny
# products and bash takes shift counts mod 64.
rt_f64_round_pack() {
  local s=$1 e=$2 m=$3 sk=$4 d
  if (( m == 0 && sk == 0 )); then
    R0=$(( s << 63 ))
    return 0
  fi
  while (( m < 1 << 53 )); do
    (( m <<= 1, e -= 1 ))
  done
  while (( m >= 1 << 54 )); do
    (( sk |= m & 1, m >>= 1, e += 1 ))
  done
  if (( e < -1022 )); then
    (( d = -1022 - e ))
    if (( d > 54 )); then d=54; fi
    (( sk |= (m & ((1 << d) - 1)) != 0, m >>= d, e = -1022 ))
  fi
  if (( (m & 1) && (sk || (m & 2)) )); then
    (( m = (m >> 1) + 1 ))
  else
    (( m >>= 1 ))
  fi
  if (( m == 1 << 53 )); then
    (( m >>= 1, e += 1 ))
  fi
  if (( e > 1023 )); then
    R0=$(( (s << 63) | 0x7ff0000000000000 ))
    return 0
  fi
  # (e+1022)<<52 + m packs normal, subnormal (e=-1022 -> 0 + m), and the
  # rounded-up-to-min-normal case in one expression.
  R0=$(( (s << 63) | (((e + 1022) << 52) + m) ))
  return 0
}
