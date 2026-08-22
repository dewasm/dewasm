# requires: rt/trap, table/check_range, table/slice
def copy(dst, other, src, len)
  Rt.trap("out of bounds table access") if src + len > other.size
  check_range(dst, len)
  return if len == 0
  @slots[dst, len] = other.slice(src, len)
end
