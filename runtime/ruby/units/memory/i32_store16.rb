# requires: rt/trap
def i32_store16(a, v) = (a &= M32; Rt.trap("out of bounds memory access") if a + 2 > @size; @buffer.set_value(:u16, a, v & 0xffff))
