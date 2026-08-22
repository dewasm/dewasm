@staticmethod
def f64_bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]
