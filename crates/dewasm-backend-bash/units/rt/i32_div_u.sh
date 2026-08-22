# requires: rt/trap
rt_i32_div_u() {
  if (( $2 == 0 )); then rt_trap 'integer divide by zero'; return $?; fi
  R0=$(( $1 / $2 ))
  return 0
}
