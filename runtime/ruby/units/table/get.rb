# requires: rt/trap
def get(i)
  Rt.trap("out of bounds table access") if i >= @slots.size
  @slots[i]
end
