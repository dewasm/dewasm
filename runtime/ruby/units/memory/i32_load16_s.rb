# requires: memory/i32_load16_u, rt/sext
def i32_load16_s(a) = Rt.sext(i32_load16_u(a), 16, M32)
