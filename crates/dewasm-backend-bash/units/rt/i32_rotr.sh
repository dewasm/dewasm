# See rt/i32_rotl for why r == 0 is safe without a special case.
rt_i32_rotr() {
  local a=$1 r=$(( $2 & 31 ))
  R0=$(( ((a >> r) | (a << (32 - r))) & 0xffffffff ))
  return 0
}
