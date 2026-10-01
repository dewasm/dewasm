# i32 values are below 2^32, so shifts by up to 32 never hit Bash's modulo-64 shift wraparound.
# So r == 0 needs no special case.
rt_i32_rotl() {
  local a=$1 r=$(( $2 & 31 ))
  R0=$(( ((a << r) | (a >> (32 - r))) & 0xffffffff ))
  return 0
}
