# requires: rt/trap
def iwlb(a) = (a &= M32; Rt.trap("out of bounds memory access") if a + 1 > @size; @buffer.get_value(:S8, a) & M32)
