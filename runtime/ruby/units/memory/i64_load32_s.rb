# requires: memory/i64_load32_u, rt/sext
def i64_load32_s(a) = Rt.sext(i64_load32_u(a), 32, M64)
