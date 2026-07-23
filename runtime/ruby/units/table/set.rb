# requires: rt/trap
def set(i, type_key, func)
  Rt.trap("out of bounds table access") if i >= @funcs.size
  @types[i] = type_key
  @funcs[i] = func
end
