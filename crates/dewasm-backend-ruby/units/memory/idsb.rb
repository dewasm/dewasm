# requires: rt/trap
def idsb(a, v) = (a &= M32; Rt.trap("out of bounds memory access") if a + 1 > @size; @buffer.set_value(:U8, a, v & 0xff))
