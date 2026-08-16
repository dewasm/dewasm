# requires: rt/trap
def idla(a, b) = (a = (a + b) & M32; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.get_value(:u64, a))
