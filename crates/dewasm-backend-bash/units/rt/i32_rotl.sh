# i32 values stay below 2^32, so shifts by up to 32 never hit bash's mod-64 shift-count wraparound and r == 0 needs no special case.
rt_i32_rotl() {
  local a=$1 r=$(( $2 & 31 ))
  R0=$(( ((a << r) | (a >> (32 - r))) & 0xffffffff ))
  return 0
}
