# requires: rt/trap
# tab_copy <dst-base> <src-base> <d> <s> <n>; copies function command
# names and their structural type keys between (or within) tables. The
# loop is direction-aware so overlapping ranges in a single table copy
# correctly; the two bases may name the same table.
tab_copy() {
  local -n __dt=$1 __dty=${1}ty __dsz=${1}sz
  local -n __st=$2 __sty=${2}ty __ssz=${2}sz
  local d=$3 s=$4 n=$5 i
  if (( s + n > __ssz )); then
    rt_trap 'out of bounds table access'
    return $?
  fi
  if (( d + n > __dsz )); then
    rt_trap 'out of bounds table access'
    return $?
  fi
  if (( d <= s )); then
    for (( i = 0; i < n; i++ )); do
      __dt[d+i]=${__st[s+i]}
      __dty[d+i]=${__sty[s+i]}
    done
  else
    for (( i = n - 1; i >= 0; i-- )); do
      __dt[d+i]=${__st[s+i]}
      __dty[d+i]=${__sty[s+i]}
    done
  fi
  return 0
}
