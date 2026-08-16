# requires: memory/i32_load8_ua, rt/sext
def i32_load8_sa(self, a, b):
    return Rt.sext(self.i32_load8_ua(a, b), 8, Rt.M32)
