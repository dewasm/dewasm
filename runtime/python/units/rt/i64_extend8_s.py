# requires: rt/sext
@staticmethod
def i64_extend8_s(x):
    return Rt.sext(x, 8, Rt.M64)
