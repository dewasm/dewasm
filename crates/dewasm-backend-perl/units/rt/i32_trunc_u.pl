# requires: rt/trap
use POSIX ();
sub i32_trunc_u {
    my $x = $_[0];
    Rt::trap('invalid conversion to integer') if $x != $x;
    Rt::trap('integer overflow') unless $x > -1.0 && $x < 4294967296.0;
    return POSIX::trunc($x) & 0xFFFFFFFF;
}
