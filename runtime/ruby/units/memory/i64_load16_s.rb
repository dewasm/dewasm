# requires: rt/trap
def i64_load16_s(a) = (Rt.trap("out of bounds memory access") if a + 2 > @size; @buffer.get_value(:s16, a) & M64)
