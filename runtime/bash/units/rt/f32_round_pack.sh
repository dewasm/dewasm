# The f32 sibling of rt/f64_round_pack: R0 = (-1)^s * m * 2^(e-24) as a
# u32 pattern, RNE. Same contract; m may be as large as 2^63-1 (integer
# conversions), which the right-normalize jam loop absorbs.
rt_f32_round_pack() {
  local s=$1 e=$2 m=$3 sk=$4 d
  if (( m == 0 && sk == 0 )); then
    R0=$(( s << 31 ))
    return 0
  fi
  while (( m < 1 << 24 )); do
    (( m <<= 1, e -= 1 ))
  done
  while (( m >= 1 << 25 )); do
    (( sk |= m & 1, m >>= 1, e += 1 ))
  done
  if (( e < -126 )); then
    (( d = -126 - e ))
    if (( d > 25 )); then d=25; fi
    (( sk |= (m & ((1 << d) - 1)) != 0, m >>= d, e = -126 ))
  fi
  if (( (m & 1) && (sk || (m & 2)) )); then
    (( m = (m >> 1) + 1 ))
  else
    (( m >>= 1 ))
  fi
  if (( m == 1 << 24 )); then
    (( m >>= 1, e += 1 ))
  fi
  if (( e > 127 )); then
    R0=$(( (s << 31) | 0x7f800000 ))
    return 0
  fi
  R0=$(( (s << 31) | (((e + 126) << 23) + m) ))
  return 0
}
