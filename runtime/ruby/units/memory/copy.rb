def copy(dst, src, len)
  check(dst, len)
  check(src, len)
  return if len == 0
  @bytes[dst, len] = @bytes.byteslice(src, len)
end
