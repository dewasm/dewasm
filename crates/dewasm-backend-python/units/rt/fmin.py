@staticmethod
def fmin(a, b):
    if a != a or b != b:
        return float("nan")
    if a < b:
        return a
    if b < a:
        return b
    if a == 0.0 and b == 0.0:
        return -0.0 if (math.copysign(1.0, a) < 0 or math.copysign(1.0, b) < 0) else 0.0
    return a
