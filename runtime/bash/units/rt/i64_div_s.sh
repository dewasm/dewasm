# requires: rt/trap
# INT64_MIN / -1 must be caught before the native division: on x86 hosts
# the underlying idiv raises SIGFPE and kills the shell.
rt_i64_div_s() {
  if (( $2 == 0 )); then rt_trap 'integer divide by zero'; return $?; fi
  if (( $1 == 1 << 63 && $2 == -1 )); then rt_trap 'integer overflow'; return $?; fi
  R0=$(( $1 / $2 ))
  return 0
}
