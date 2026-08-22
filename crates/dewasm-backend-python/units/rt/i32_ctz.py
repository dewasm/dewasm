@staticmethod
def i32_ctz(x):
    return 32 if x == 0 else (x & -x).bit_length() - 1
