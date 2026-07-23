# requires: mem/check
mem_fill() {
  local -n __m=${1}mem
  local d=$2 v=$(( $3 & 0xff )) n=$4 i
  mem_check "$1" "$d" "$n" || return $?
  for (( i = 0; i < n; i++ )); do
    __m[d+i]=$v
  done
  return 0
}
