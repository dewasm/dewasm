# requires: rt/trap
def call(i, type_idx, *args)
  Rt.trap("undefined element") if i >= @funcs.size
  func = @funcs[i]
  Rt.trap("uninitialized element") if func.nil?
  Rt.trap("indirect call type mismatch") unless @types[i] == type_idx
  func.call(*args)
end
