def read_string(ptr, len)
  check(ptr, len)
  @buffer.get_string(ptr, len)
end
