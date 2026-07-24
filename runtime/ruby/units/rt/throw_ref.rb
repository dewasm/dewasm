# requires: rt/trap, rt/wasm_exception
def throw_ref(exn)
  trap("null exception reference") if exn.nil?
  raise exn
end
