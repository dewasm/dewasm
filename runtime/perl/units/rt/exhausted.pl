# requires: rt/trap
# The call-depth cutoff (ADR-55); generated functions check $Rt::DEPTH
# against $Rt::LIMIT and land here.
sub exhausted {
    Rt::trap('call stack exhausted');
}
