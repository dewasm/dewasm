# requires: rt/trap
# mem_check <prefix> <addr> <len>: trap unless [addr, addr+len) fits.
mem_check() {
  local -n __pages=${1}pages
  if (( $2 + $3 > __pages * 65536 )); then
    rt_trap 'out of bounds memory access'
    return $?
  fi
  return 0
}
