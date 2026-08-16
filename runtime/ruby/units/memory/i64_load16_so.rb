# requires: rt/trap, rt/m64
def i64_load16_so(a, off) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 2 > @size; Rt.m64(@buffer.get_value(:s16, a)))
