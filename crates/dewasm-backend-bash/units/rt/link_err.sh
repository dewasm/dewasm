# A failed import resolution at instantiation time: a missing import, or one of the wrong kind.
# It is kept distinct from `rt_trap` and `rt_exit`.
# So a harness can tell "the module failed to link" apart from "it linked and then trapped/exited".
# The status chain codes are:
# 133 = `proc_exit` (`rt_exit`), 134 = trap (`rt_trap`), 135 = link error.
# 135 is 128 + SIGBUS in the signal convention.
# No child process raises SIGBUS in a generated module, so the theoretical collision is accepted.
rt_link_err() {
  TRAP_MSG=$1
  return 135
}
