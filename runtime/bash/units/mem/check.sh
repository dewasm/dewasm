# requires: rt/trap
# Linear memory (<prefix>mem) is an associative array, one byte per element
# (ADR-51). Assoc subscripts are literal strings, never arithmetic-evaluated,
# so every mem unit pre-expands its key to a plain integer var (`__m[$k]`, not
# `__m[a+1]`). Keys are canonical decimal integers because they all come from
# $(( )); "07" and "7" would be distinct keys otherwise. Missing keys read as 0
# in arithmetic (no `set -u` in generated scripts), so zero-init and grow stay
# free, exactly as the former indexed representation.
# mem_check <prefix> <addr> <len>: trap unless [addr, addr+len) fits.
mem_check() {
  local -n __pages=${1}pages
  if (( $2 + $3 > __pages * 65536 )); then
    rt_trap 'out of bounds memory access'
    return $?
  fi
  return 0
}
