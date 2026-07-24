# requires: rt/exit
# WASI 0.2's exit conveys only ok/err; codes collapse to 0/1.
def p2_cli_exit_exit(status)
  raise Rt::Exit.new(status[0] == :ok ? 0 : 1)
end
