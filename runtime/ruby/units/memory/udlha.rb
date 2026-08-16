# requires: rt/trap
def udlha(a, b) = (a = (a + b) & M32; Rt.trap("out of bounds memory access") if a + 2 > @size; @buffer.get_value(:u16, a))
