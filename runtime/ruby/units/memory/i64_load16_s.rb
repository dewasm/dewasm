# requires: memory/i64_load16_u, rt/sext
def i64_load16_s(a) = Rt.sext(i64_load16_u(a), 16, M64)
