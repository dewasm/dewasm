# requires: memory/i64_load8_u, rt/sext
def i64_load8_s(self, a):
    return Rt.sext(self.i64_load8_u(a), 8, Rt.M64)
