@staticmethod
def trunc_sat(x, lo, hi):
    if x != x:
        return 0
    if math.isinf(x):
        t = hi if x > 0 else lo
    else:
        t = math.trunc(x)
    return lo if t < lo else (hi if t > hi else t)
