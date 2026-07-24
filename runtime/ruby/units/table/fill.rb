# requires: rt/trap, table/check_range
def fill(dst, val, len)
  check_range(dst, len)
  return if len == 0
  @slots.fill(val, dst, len)
end
