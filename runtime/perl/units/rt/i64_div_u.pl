# requires: rt/trap
# See Rt::i32_div_u: the remainder-adjusted numerator divides exactly, and
# perl's `/` returns the exact integer for even unsigned divisions
# (measured, ADR-55) — NV division of the raw 64-bit operands would lose
# precision.
sub i64_div_u {
    my ($a, $b) = @_;
    Rt::trap('integer divide by zero') if $b == 0;
    return ($a - $a % $b) / $b;
}
