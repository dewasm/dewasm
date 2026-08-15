# requires: rt/trap
def f64_storeo(a, off, v) = (a += off; Rt.trap("out of bounds memory access") if a + 8 > @size; @buffer.set_value(:f64, a, v))
