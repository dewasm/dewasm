# requires: rt/sext
@staticmethod
def i64_extend16_s(x):
    return Rt.sext(x, 16, Rt.M64)
