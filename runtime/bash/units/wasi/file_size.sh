# wasi_file_size <path>: byte length of the file at <path>, in R1.
# Same NUL-splitting `read -d ''` loop as file_slurp but only counts: each successful read consumed a chunk plus its NUL delimiter (+1); the final nonzero read leaves the trailing bytes after the last NUL.
# R0 is the errno.
wasi_file_size() {
  local __path=$1
  local LC_ALL=C
  local __chunk __n=0
  while IFS= read -r -d '' __chunk; do
    (( __n += ${#__chunk} + 1 ))
  done < "$__path"
  (( __n += ${#__chunk} ))
  R1=$__n
  R0=0
  return 0
}
