# requires: rt/trap
def i64_store32a(a, b, v) = (a = (a + b) & M32; Rt.trap("out of bounds memory access") if a + 4 > @size; @buffer.set_value(:u32, a, v & M32))
