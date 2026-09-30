# requires: rt/quiet_nan
# C99 nearbyint under the default rounding mode is round-half-to-even and preserves signed zeros.
# That is exactly wasm's nearest (measured).
use POSIX ();
sub fnearest {
    my $x = $_[0];
    return Rt::quiet_nan($x) if $x != $x;
    return POSIX::nearbyint($x);
}
