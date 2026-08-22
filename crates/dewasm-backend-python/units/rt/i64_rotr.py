# requires: rt/_module
@staticmethod
def i64_rotr(a, b):
    r = b & 63
    return ((a >> r) | (a << (64 - r))) & Rt.M64 if r else a
