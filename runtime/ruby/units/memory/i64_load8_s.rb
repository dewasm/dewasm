# requires: rt/trap, rt/m64
def i64_load8_s(a) = (Rt.trap("out of bounds memory access") if a + 1 > @size; Rt.m64(@buffer.get_value(:S8, a)))
