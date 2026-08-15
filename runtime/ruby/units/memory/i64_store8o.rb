# requires: rt/trap
def i64_store8o(a, off, v) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 1 > @size; @buffer.set_value(:U8, a, v & 0xff))
