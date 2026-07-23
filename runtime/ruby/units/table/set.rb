# requires: rt/trap
def set(i, type_idx, func)
  Rt.trap("out of bounds table access") if i >= @funcs.size
  @types[i] = type_idx
  @funcs[i] = func
end
