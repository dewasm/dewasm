# r == 0 must be special-cased: bash takes shift counts mod 64, so the complementary shift by 64 would be a shift by 0, not a clear-out.
rt_i64_rotl() {
  local a=$1 r=$(( $2 & 63 ))
  if (( r == 0 )); then
    R0=$a
    return 0
  fi
  R0=$(( (a << r) | ((a >> (64 - r)) & ~(-1 << r)) ))
  return 0
}
