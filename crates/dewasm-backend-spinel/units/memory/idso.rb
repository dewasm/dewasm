# requires: rt/trap
def idso(a, off, v) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.set_value(:s64, a, v))
