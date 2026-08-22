# requires: rt/trap
# b == -1 short-circuits to 0: INT64_MIN % -1 would SIGFPE on x86 hosts.
rt_i64_rem_s() {
  if (( $2 == 0 )); then rt_trap 'integer divide by zero'; return $?; fi
  if (( $2 == -1 )); then
    R0=0
    return 0
  fi
  R0=$(( $1 % $2 ))
  return 0
}
