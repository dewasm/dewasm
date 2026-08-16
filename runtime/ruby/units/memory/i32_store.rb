# requires: rt/trap
def i32_store(a, v) = (a &= M32; Rt.trap("out of bounds memory access") if a + 4 > @size; @buffer.set_value(:u32, a, v & M32))
