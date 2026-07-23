# requires: rt/f64_round_pack
# A u64 with the top bit set is halved exactly; the lost bit 0 can never
# become the guard bit (the mantissa still needs >= 9 right shifts), so
# feeding it as sticky is exact.
rt_f64_convert_i64_u() {
  local v=$1
  if (( v < 0 )); then
    rt_f64_round_pack 0 54 $(( (v >> 1) & 0x7fffffffffffffff )) $(( v & 1 ))
  else
    rt_f64_round_pack 0 53 "$v" 0
  fi
  return 0
}
