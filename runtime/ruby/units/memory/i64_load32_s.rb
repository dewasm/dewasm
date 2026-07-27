# requires: rt/trap
def i64_load32_s(a) = (Rt.trap("out of bounds memory access") if a + 4 > @size; @buffer.get_value(:s32, a) & M64)
