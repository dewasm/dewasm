# requires: rt/trap
rt_i32_rem_s() {
  local a=$(( ($1 ^ 0x80000000) - 0x80000000 )) b=$(( ($2 ^ 0x80000000) - 0x80000000 ))
  if (( b == 0 )); then rt_trap 'integer divide by zero'; return $?; fi
  R0=$(( (a % b) & 0xffffffff ))
  return 0
}
