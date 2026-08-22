# requires: rt/trap
def iwlho(a, off) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 2 > @size; @buffer.get_value(:s16, a) & M32)
