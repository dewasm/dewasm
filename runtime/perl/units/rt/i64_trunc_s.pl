# requires: rt/trap
# -2^63 is itself an exact NV and in range; the nearest NV below it already
# truncates out of range, so >= is the correct lower bound (ADR-55).
use POSIX ();
sub i64_trunc_s {
    my $x = $_[0];
    Rt::trap('invalid conversion to integer') if $x != $x;
    Rt::trap('integer overflow') unless $x >= -9223372036854775808.0 && $x < 9223372036854775808.0;
    return POSIX::trunc($x) & 0xFFFFFFFFFFFFFFFF;
}
