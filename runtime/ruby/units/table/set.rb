# requires: rt/trap
def set(i, value)
  Rt.trap("out of bounds table access") if i >= @slots.size
  @slots[i] = value
end
