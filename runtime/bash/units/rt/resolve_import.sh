# requires: rt/link_err
# rt_resolve_import <mod> <name> <kind>; kind in func|global|table|memory.
# The bash shape of the ADR-7 provider protocol (ADR-33). Resolution order:
#   (1) for funcs only, an IMPORTS[mod.name] override wins (host wiring);
#   (2) PROVIDERS[mod] names a prefix <q> that owns per-kind export maps —
#       <q>EXPORTS (funcs), <q>GLOBAL_EXPORTS, <q>TABLE_EXPORTS,
#       <q>MEMORY_EXPORTS — whose value under `name` is returned in the
#       global RESOLVED. The value's meaning is kind-specific (the caller
#       knows the kind it asked for): a func command name, a global's
#       target variable name, a table's array base name, a memory prefix.
# A name found only in a DIFFERENT kind's map is an incompatible-type link
# error. Missing everywhere leaves RESOLVED='' and returns 0, so the caller
# decides (WASI/ENOSYS fallback for WASI modules, else a link error).
rt_resolve_import() {
  local mod=$1 name=$2 kind=$3 q entry kk map
  RESOLVED=''
  if [[ $kind == func && -n ${IMPORTS[$mod.$name]-} ]]; then
    RESOLVED=${IMPORTS[$mod.$name]}
    return 0
  fi
  q=${PROVIDERS[$mod]-}
  [[ -n $q ]] || return 0
  for entry in func:EXPORTS global:GLOBAL_EXPORTS table:TABLE_EXPORTS memory:MEMORY_EXPORTS; do
    kk=${entry%%:*}
    map=$q${entry#*:}
    declare -p "$map" &>/dev/null || continue
    local -n __m=$map
    [[ -n ${__m[$name]+set} ]] || continue
    if [[ $kk == "$kind" ]]; then
      RESOLVED=${__m[$name]}
      return 0
    fi
    rt_link_err "incompatible import type for $mod.$name"
    return $?
  done
  return 0
}
