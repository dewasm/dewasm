# requires: rt/f64_round_pack
# INT64_MIN must short-circuit: bash negation of 1<<63 is a wrapping
# no-op, which would loop the normalizer forever.
rt_f64_convert_i64_s() {
  local v=$1 s=0
  if (( v == 1 << 63 )); then
    R0=$(( 0xC3E0000000000000 ))
    return 0
  fi
  if (( v < 0 )); then
    (( s = 1, v = -v ))
  fi
  rt_f64_round_pack "$s" 53 "$v" 0
  return 0
}
