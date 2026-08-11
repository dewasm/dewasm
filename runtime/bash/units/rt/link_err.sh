# A failed import resolution at instantiation time (a missing import, or
# one of the wrong kind), kept distinct from rt_trap and rt_exit so a
# harness can tell "the module failed to link" apart from "the module
# linked and then trapped/exited". The status cascade is:
#   133 = proc_exit (rt_exit), 134 = trap (rt_trap), 135 = link error.
# 135 is 128 + SIGBUS in the signal convention; no subprocess raises
# SIGBUS in a generated module, so the theoretical collision is accepted.
rt_link_err() {
  TRAP_MSG=$1
  return 135
}
