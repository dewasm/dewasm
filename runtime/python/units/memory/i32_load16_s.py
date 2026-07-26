# requires: memory/i32_load16_u, rt/sext
def i32_load16_s(self, a):
    return Rt.sext(self.i32_load16_u(a), 16, Rt.M32)
