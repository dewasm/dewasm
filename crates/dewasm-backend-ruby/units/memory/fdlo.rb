# requires: rt/trap
def fdlo(a, off) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.get_value(:f64, a))
