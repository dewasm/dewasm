# requires: rt/trap
def idlo(a, off) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.get_value(:u64, a))
