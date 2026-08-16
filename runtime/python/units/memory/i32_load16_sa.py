# requires: memory/i32_load16_ua, rt/sext
def i32_load16_sa(self, a, b):
    return Rt.sext(self.i32_load16_ua(a, b), 16, Rt.M32)
