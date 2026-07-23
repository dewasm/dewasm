# requires: memory/i32_load8_u, rt/sext
def i32_load8_s(a) = Rt.sext(i32_load8_u(a), 8, M32)
