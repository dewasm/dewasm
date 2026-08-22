# requires: rt/trap
def udlwo(a, off) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 4 > @size; @buffer.get_value(:u32, a))
