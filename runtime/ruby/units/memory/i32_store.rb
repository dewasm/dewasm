# requires: rt/trap
def i32_store(a, v) = (Rt.trap("out of bounds memory access") if a + 4 > @size; @buffer.set_value(:u32, a, v))
