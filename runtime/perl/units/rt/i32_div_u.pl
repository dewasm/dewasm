# requires: rt/trap
# Perl `/` returns the exact integer when an unsigned division is even
# (measured, ADR-55), so divide the remainder-adjusted numerator instead of
# trusting NV division of the raw operands.
sub i32_div_u {
    my ($a, $b) = @_;
    Rt::trap('integer divide by zero') if $b == 0;
    return ($a - $a % $b) / $b;
}
