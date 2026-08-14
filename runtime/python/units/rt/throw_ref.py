# requires: rt/trap, rt/wasm_exception
@staticmethod
def throw_ref(exn):
    if exn is None:
        Rt.trap("null exception reference")
    raise exn
