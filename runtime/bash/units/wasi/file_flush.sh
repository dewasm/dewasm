# wasi_file_flush <array-name> <path>: write the named byte-ordinal array to
# <path>, replacing its contents (ADR-34 whole-file buffering). A single
# `exec {fd}>` truncate-create opens the file; bytes are emitted through a
# binary-safe '\xHH' printf format built in ~4 KiB batches. R0 is the errno.
wasi_file_flush() {
  local -n __buf=$1
  local __path=$2
  local LC_ALL=C
  local __ofd __i __n=${#__buf[@]} __fmt __batch=()
  if ! exec {__ofd}>"$__path"; then
    R0=29 # EIO
    return 0
  fi
  for (( __i = 0; __i < __n; __i++ )); do
    __batch+=("$(( __buf[__i] & 0xff ))")
    if (( ${#__batch[@]} >= 4096 )); then
      printf -v __fmt '\\x%02x' "${__batch[@]}"
      printf "$__fmt" >&"$__ofd"
      __batch=()
    fi
  done
  if (( ${#__batch[@]} > 0 )); then
    printf -v __fmt '\\x%02x' "${__batch[@]}"
    printf "$__fmt" >&"$__ofd"
  fi
  exec {__ofd}>&-
  R0=0
  return 0
}
