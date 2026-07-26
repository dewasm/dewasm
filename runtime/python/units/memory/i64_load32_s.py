# requires: memory/i64_load32_u, rt/sext
def i64_load32_s(self, a):
    return Rt.sext(self.i64_load32_u(a), 32, Rt.M64)
