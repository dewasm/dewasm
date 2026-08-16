# requires: rt/trap
def fdl(a) = (a &= M32; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.get_value(:f64, a))
