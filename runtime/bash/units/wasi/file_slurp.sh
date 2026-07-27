# wasi_file_slurp <path> <array-name>: read the whole file at <path> into the
# named indexed array as byte ordinals (ADR-32 whole-file buffering). Reads with
# `read -r -d ''`, which splits on NUL: status 0 means a NUL delimiter was
# consumed (append the chunk's bytes, then a 0 for the NUL); a final nonzero read
# leaves the trailing bytes after the last NUL in the chunk. R0 is the errno.
wasi_file_slurp() {
  local __path=$1
  local -n __buf=$2
  __buf=()
  local LC_ALL=C
  local __chunk __i __b __n=0
  while IFS= read -r -d '' __chunk; do
    for (( __i = 0; __i < ${#__chunk}; __i++ )); do
      printf -v __b '%d' "'${__chunk:__i:1}"
      __buf[__n++]=$(( __b & 0xff ))
    done
    __buf[__n++]=0
  done < "$__path"
  for (( __i = 0; __i < ${#__chunk}; __i++ )); do
    printf -v __b '%d' "'${__chunk:__i:1}"
    __buf[__n++]=$(( __b & 0xff ))
  done
  R0=0
  return 0
}
