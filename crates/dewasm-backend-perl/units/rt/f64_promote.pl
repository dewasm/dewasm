# requires: rt/quiet_nan
sub f64_promote {
    return $_[0] != $_[0] ? Rt::quiet_nan($_[0]) : $_[0];
}
