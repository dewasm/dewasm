# requires: rt/trap
def fdsa(a, b, v) = (a = (a + b) & M32; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.set_value(:f64, a, v))
