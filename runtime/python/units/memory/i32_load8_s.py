# requires: memory/i32_load8_u, rt/sext
def i32_load8_s(self, a):
    return Rt.sext(self.i32_load8_u(a), 8, Rt.M32)
