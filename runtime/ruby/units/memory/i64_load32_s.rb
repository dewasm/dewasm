# requires: rt/trap, rt/m64
def i64_load32_s(a) = (Rt.trap("out of bounds memory access") if a + 4 > @size; Rt.m64(@buffer.get_value(:s32, a)))
