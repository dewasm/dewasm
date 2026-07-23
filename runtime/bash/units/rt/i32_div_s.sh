# requires: rt/trap
rt_i32_div_s() {
  local a=$(( ($1 ^ 0x80000000) - 0x80000000 )) b=$(( ($2 ^ 0x80000000) - 0x80000000 ))
  if (( b == 0 )); then rt_trap 'integer divide by zero'; return $?; fi
  if (( a == -2147483648 && b == -1 )); then rt_trap 'integer overflow'; return $?; fi
  R0=$(( (a / b) & 0xffffffff ))
  return 0
}
