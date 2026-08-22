# requires: rt/f64_round_pack
# Guard/round/sticky live in the low 3 bits (<<3); the alignment shift jams shifted-out bits into bit 0, which is sound because a shift by d >= 2 keeps the difference >= 2^53 (right-normalization only), and d <= 1 loses no bits at all (mb<<3 has 3 trailing zeros).
rt_f64_add() {
  local a=$1 b=$2 pa pb sa sb ea eb ma mb d x y m t
  (( pa = a & 0x7fffffffffffffff, pb = b & 0x7fffffffffffffff ))
  if (( pa > 0x7ff0000000000000 || pb > 0x7ff0000000000000 )); then
    R0=$(( 0x7ff8000000000000 ))
    return 0
  fi
  if (( pa == 0x7ff0000000000000 || pb == 0x7ff0000000000000 )); then
    if (( pa == pb && (a ^ b) < 0 )); then
      R0=$(( 0x7ff8000000000000 ))
    elif (( pa == 0x7ff0000000000000 )); then
      R0=$(( a ))
    else
      R0=$(( b ))
    fi
    return 0
  fi
  if (( pa == 0 && pb == 0 )); then
    R0=$(( a & b ))
    return 0
  fi
  if (( pb == 0 )); then
    R0=$(( a ))
    return 0
  fi
  if (( pa == 0 )); then
    R0=$(( b ))
    return 0
  fi
  if (( pa < pb )); then
    (( t = a, a = b, b = t, t = pa, pa = pb, pb = t ))
  fi
  (( sa = (a >> 63) & 1, sb = (b >> 63) & 1 ))
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
  (( d = ea - eb, x = ma << 3, y = mb << 3 ))
  if (( d > 56 )); then
    y=1
  elif (( d > 0 )); then
    (( y = (y >> d) | ((y & ((1 << d) - 1)) != 0) ))
  fi
  if (( sa == sb )); then
    (( m = x + y ))
  else
    (( m = x - y ))
  fi
  if (( m == 0 )); then
    R0=0
    return 0
  fi
  rt_f64_round_pack "$sa" $(( ea - 2 )) "$m" 0
  return 0
}
