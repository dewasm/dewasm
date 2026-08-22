def copy(dst, src, len)
  check(dst, len)
  check(src, len)
  return if len == 0
  @buffer.copy(@buffer, dst, len, src)
end
