# requires: rt/trap
def ids(a, v) = (a &= M32; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.set_value(:s64, a, v))
