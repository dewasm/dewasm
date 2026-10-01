# requires: mem/i64_store
# Clock ID 0 is `realtime`; IDs 1-3 (`monotonic` and the CPU-time clocks) also read `realtime`.
# Pure Bash has no monotonic clock source (documented deviation).
wasi_clock_time_get() {
  local __p=$1 __id=$2 __out=$4
  if (( __id < 0 || __id > 3 )); then
    R0=28 # EINVAL
    return 0
  fi
  local __s=${EPOCHREALTIME%%.*} __us=${EPOCHREALTIME##*.}
  mem_i64_store "$__p" "$__out" $(( __s * 1000000000 + 10#$__us * 1000 )) || return $?
  R0=0
  return 0
}
