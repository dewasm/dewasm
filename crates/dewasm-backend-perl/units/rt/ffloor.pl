# requires: rt/quiet_nan
use POSIX ();
sub ffloor {
    my $x = $_[0];
    return Rt::quiet_nan($x) if $x != $x;
    return POSIX::floor($x);
}
