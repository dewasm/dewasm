# requires: rt/quiet_nan
@staticmethod
def fsqrt(x):
    if x != x:
        return Rt.quiet_nan(x)
    if x < 0.0:
        return float("nan")
    if x == 0.0:
        return x
    return math.sqrt(x)
