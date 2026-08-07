# requires: rt/quiet_nan
# C99 ceil handles infinities and signed zeros; NaNs are quieted by hand
# because the spec requires it and the C round-trip may not.
use POSIX ();
sub fceil {
    my $x = $_[0];
    return Rt::quiet_nan($x) if $x != $x;
    return POSIX::ceil($x);
}
