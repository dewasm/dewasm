# requires: rt/trap
def fdla(a, b) = (a = (a + b) & M32; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.get_value(:f64, a))
