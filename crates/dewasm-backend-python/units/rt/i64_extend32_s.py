# requires: rt/sext
@staticmethod
def i64_extend32_s(x):
    return Rt.sext(x, 32, Rt.M64)
