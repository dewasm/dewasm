# requires: rt/exit
def wasi_proc_exit(self, code):
    raise Rt.Exit(code)
