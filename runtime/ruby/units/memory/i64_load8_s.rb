# requires: memory/i64_load8_u, rt/sext
def i64_load8_s(a) = Rt.sext(i64_load8_u(a), 8, M64)
