@staticmethod
def i32_clz(x):
    return 32 if x == 0 else 32 - x.bit_length()
