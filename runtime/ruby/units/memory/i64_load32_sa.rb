# requires: rt/trap, rt/m64
def i64_load32_sa(a, b) = (a = (a + b) & M32; Rt.trap("out of bounds memory access") if a + 4 > @size; Rt.m64(@buffer.get_value(:s32, a)))
