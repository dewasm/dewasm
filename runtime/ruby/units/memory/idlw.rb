# requires: rt/trap, rt/m64
def idlw(a) = (a &= M32; Rt.trap("out of bounds memory access") if a + 4 > @size; Rt.m64(@buffer.get_value(:s32, a)))
