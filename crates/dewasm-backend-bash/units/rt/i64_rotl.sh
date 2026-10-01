# r == 0 must be special-cased, since Bash takes shift counts modulo 64.
# So the second shift, by 64, would be a shift by 0, not a clear-out.
rt_i64_rotl() {
  local a=$1 r=$(( $2 & 63 ))
  if (( r == 0 )); then
    R0=$a
    return 0
  fi
  R0=$(( (a << r) | ((a >> (64 - r)) & ~(-1 << r)) ))
  return 0
}
