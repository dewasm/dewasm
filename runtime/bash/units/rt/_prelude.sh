# Root prelude: shared runtime state.
# Trap protocol: a helper that traps sets TRAP_MSG and returns status 134; every other unit function returns 0 explicitly (a trailing arithmetic statement would leak status 1).
TRAP_MSG=''
