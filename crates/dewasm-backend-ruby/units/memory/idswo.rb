# requires: rt/trap
def idswo(a, off, v) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 4 > @size; @buffer.set_value(:u32, a, v & M32))
