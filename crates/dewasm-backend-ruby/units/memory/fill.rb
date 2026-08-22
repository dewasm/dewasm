def fill(dst, val, len)
  check(dst, len)
  return if len == 0
  @buffer.clear(val & 0xff, dst, len)
end
