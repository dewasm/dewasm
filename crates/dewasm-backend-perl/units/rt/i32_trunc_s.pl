# requires: rt/trap
# The range check runs on the double itself: every NV strictly inside
# (-2^31 - 1, 2^31) truncates in range, and the bounds are exact NVs.
use POSIX ();
sub i32_trunc_s {
    my $x = $_[0];
    Rt::trap('invalid conversion to integer') if $x != $x;
    Rt::trap('integer overflow') unless $x > -2147483649.0 && $x < 2147483648.0;
    return POSIX::trunc($x) & 0xFFFFFFFF;
}
