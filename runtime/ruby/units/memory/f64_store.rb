# requires: rt/trap
def f64_store(a, v) = (a &= M32; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.set_value(:f64, a, v))
