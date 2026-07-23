# requires: mem/i64_store
# EPOCHREALTIME has microsecond granularity.
wasi_clock_res_get() {
  mem_i64_store "$1" "$3" 1000 || return $?
  R0=0
  return 0
}
