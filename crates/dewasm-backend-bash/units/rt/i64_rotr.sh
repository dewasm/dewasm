# See rt/i64_rotl for the r == 0 special case.
rt_i64_rotr() {
  local a=$1 r=$(( $2 & 63 ))
  if (( r == 0 )); then
    R0=$a
    return 0
  fi
  R0=$(( ((a >> r) & ~(-1 << (64 - r))) | (a << (64 - r)) ))
  return 0
}
