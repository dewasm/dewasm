# requires: rt/trap
def i32_storeo(a, off, v) = (a += off; Rt.trap("out of bounds memory access") if a + 4 > @size; @buffer.set_value(:u32, a, v))
