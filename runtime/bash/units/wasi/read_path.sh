# requires: mem/check
# Decodes `len` guest-memory bytes at `ptr` into a bash string (returned in
# R1). A path cannot hold a NUL byte (bash strings cannot either), so an
# embedded NUL is rejected with EILSEQ (25), the illegal-byte-sequence errno the
# conformance suite accepts for a NUL-bearing path (Ruby uses EPERM but
# never faces this case, having no shared byte-buffer decode). The path is
# rebuilt through a single '\xHH' printf format (the binary-safe technique
# fd_write uses), so arbitrary non-NUL bytes survive.
wasi_read_path() {
  local __p=$1 __ptr=$2 __len=$3
  local -n __m=${__p}mem
  mem_check "$__p" "$__ptr" "$__len" || return $?
  local LC_ALL=C
  local __i __b __hh __fmt='' __mk
  for (( __i = 0; __i < __len; __i++ )); do
    __mk=$(( __ptr + __i ))
    __b=$(( __m[$__mk] & 0xff ))
    if (( __b == 0 )); then
      R0=25 # EILSEQ: embedded NUL in path
      return 0
    fi
    printf -v __hh '\\x%02x' "$__b"
    __fmt+=$__hh
  done
  printf -v R1 "$__fmt"
  R0=0
  return 0
}
