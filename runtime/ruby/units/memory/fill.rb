def fill(dst, val, len)
  check(dst, len)
  return if len == 0
  @bytes[dst, len] = ((val & 0xff).chr * len).b
end
