# requires: rt/trap
def udlb(a) = (a &= M32; Rt.trap("out of bounds memory access") if a + 1 > @size; @buffer.get_value(:U8, a))
