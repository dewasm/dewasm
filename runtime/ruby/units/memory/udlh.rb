# requires: rt/trap
def udlh(a) = (a &= M32; Rt.trap("out of bounds memory access") if a + 2 > @size; @buffer.get_value(:u16, a))
