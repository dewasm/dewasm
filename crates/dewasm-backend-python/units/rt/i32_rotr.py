# requires: rt/_module
@staticmethod
def i32_rotr(a, b):
    r = b & 31
    return ((a >> r) | (a << (32 - r))) & Rt.M32 if r else a
