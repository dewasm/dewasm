# requires: rt/trap, rt/m64
def idlba(a, b) = (a = (a + b) & M32; Rt.trap("out of bounds memory access") if a + 1 > @size; Rt.m64(@buffer.get_value(:S8, a)))
