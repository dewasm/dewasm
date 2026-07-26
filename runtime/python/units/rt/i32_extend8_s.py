# requires: rt/sext
@staticmethod
def i32_extend8_s(x):
    return Rt.sext(x, 8, Rt.M32)
