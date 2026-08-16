# requires: rt/trap
def udlbo(a, off) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 1 > @size; @buffer.get_value(:U8, a))
