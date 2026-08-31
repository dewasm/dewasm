# requires: rt/trap
# tab_init <table-base> <elem-base> <dst> <src> <len>; copies function command names, their structural type keys, and their tail commands from a staged element array into a table.
# A dropped element segment is an empty array, so its length check traps like the spec asks.
tab_init() {
  local -n __t=$1 __ty=${1}ty __tl=${1}tl __sz=${1}sz
  local -n __e=$2 __ety=${2}ty __etl=${2}tl
  local d=$3 s=$4 n=$5 i
  if (( s + n > ${#__e[@]} )); then
    rt_trap 'out of bounds table access'
    return $?
  fi
  if (( d + n > __sz )); then
    rt_trap 'out of bounds table access'
    return $?
  fi
  for (( i = 0; i < n; i++ )); do
    __t[d+i]=${__e[s+i]}
    __ty[d+i]=${__ety[s+i]}
    __tl[d+i]=${__etl[s+i]}
  done
  return 0
}
