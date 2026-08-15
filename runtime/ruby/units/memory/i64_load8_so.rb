# requires: rt/trap, rt/m64
def i64_load8_so(a, off) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 1 > @size; Rt.m64(@buffer.get_value(:S8, a)))
