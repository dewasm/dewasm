# requires: rt/trap
def udlho(a, off) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 2 > @size; @buffer.get_value(:u16, a))
