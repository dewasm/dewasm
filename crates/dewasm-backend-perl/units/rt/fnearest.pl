# requires: rt/quiet_nan
# C99 `nearbyint` under the default rounding mode is round-half-to-even (measured).
# It also preserves signed zeros (measured); that is exactly wasm's nearest.
use POSIX ();
sub fnearest {
    my $x = $_[0];
    return Rt::quiet_nan($x) if $x != $x;
    return POSIX::nearbyint($x);
}
