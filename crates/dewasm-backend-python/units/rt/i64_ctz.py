@staticmethod
def i64_ctz(x):
    return 64 if x == 0 else (x & -x).bit_length() - 1
