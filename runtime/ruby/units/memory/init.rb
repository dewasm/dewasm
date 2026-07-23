# requires: rt/trap
# Also used to initialize active data segments at instantiation time.
def init(dst, data, src, len)
  Rt.trap("out of bounds memory access") if src + len > data.bytesize
  check(dst, len)
  return if len == 0
  @bytes[dst, len] = data.byteslice(src, len)
end
