# requires: rt/trap
def f64_storeo(a, off, v) = (a = (a & M32) + off; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.set_value(:f64, a, v))
