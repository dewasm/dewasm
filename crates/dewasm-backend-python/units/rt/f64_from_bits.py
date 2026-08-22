@staticmethod
def f64_from_bits(b):
    return struct.unpack("<d", struct.pack("<Q", b))[0]
