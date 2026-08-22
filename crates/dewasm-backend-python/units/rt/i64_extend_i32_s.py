# requires: rt/s32
@staticmethod
def i64_extend_i32_s(x):
    return Rt.s32(x) & Rt.M64
