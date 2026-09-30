# Root prelude: shared runtime state.
# Trap protocol: a helper that traps sets TRAP_MSG and returns status 134.
# Every other unit function returns 0 explicitly.
# A trailing arithmetic statement would make the function return status 1.
TRAP_MSG=''
