def read_string(ptr, len)
  check(ptr, len)
  @bytes.byteslice(ptr, len)
end
