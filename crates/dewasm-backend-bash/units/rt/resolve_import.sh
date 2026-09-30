# requires: rt/link_err
# `rt_resolve_import <mod> <name> <kind>`; `kind` in `func|global|table|memory`.
# The Bash shape of the import-provider protocol.
# Resolution order:
# (1) For functions only, an `IMPORTS[mod.name]` override set by the host wins.
# (2) `PROVIDERS[mod]` names a prefix `<q>` that owns per-kind export maps.
# The function map is `<q>EXPORTS`.
# The other maps are `<q>GLOBAL_EXPORTS`, `<q>TABLE_EXPORTS`, and `<q>MEMORY_EXPORTS`.
# A map's value under `name` is returned in the global RESOLVED.
# The value's meaning is kind-specific, since the caller knows the kind it asked for.
# For a function the value is a command name, and for a global it is its target variable name.
# For a table it is an array base name, and for a memory it is a prefix.
# A name found only in a DIFFERENT kind's map is a link error, since its type does not match.
# Missing everywhere leaves RESOLVED='' and returns 0, so the caller decides.
# The caller falls back to WASI/ENOSYS for WASI modules, else raises a link error.
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
