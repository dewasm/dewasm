# requires: rt/trap, rt/m64
def idso(a, off, v) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.set_value(:u64, a, Rt.m64(v)))
