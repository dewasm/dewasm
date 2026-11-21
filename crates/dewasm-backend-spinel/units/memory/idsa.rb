# requires: rt/trap
def idsa(a, b, v) = (a = (a + b) & M32; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.set_value(:s64, a, v))
