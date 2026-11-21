# Flipping the sign bit maps the unsigned order onto the signed one.
def i64_lt_u(a, b) = (a ^ I64_MIN) < (b ^ I64_MIN)
