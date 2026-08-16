# requires: rt/trap
def i32_store8a(a, b, v) = (a = (a + b) & M32; Rt.trap("out of bounds memory access") if a + 1 > @size; @buffer.set_value(:U8, a, v & 0xff))
