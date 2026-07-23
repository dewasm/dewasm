# requires: rt/exit
def wasi_proc_exit(code)
  raise Rt::Exit, code
end
