# requires: rt/trap
def iwsha(a, b, v) = (a = (a + b) & M32; Rt.trap("out of bounds memory access") if a + 2 > @size; @buffer.set_value(:u16, a, v & 0xffff))
