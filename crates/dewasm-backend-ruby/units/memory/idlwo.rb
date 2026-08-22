# requires: rt/trap, rt/m64
def idlwo(a, off) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 4 > @size; Rt.m64(@buffer.get_value(:s32, a)))
