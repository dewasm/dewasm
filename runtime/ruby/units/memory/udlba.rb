# requires: rt/trap
def udlba(a, b) = (a = (a + b) & M32; Rt.trap("out of bounds memory access") if a + 1 > @size; @buffer.get_value(:U8, a))
