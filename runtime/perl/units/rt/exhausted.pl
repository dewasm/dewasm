# requires: rt/trap
# The call-depth cutoff; generated functions check $Rt::DEPTH
# against $Rt::LIMIT and land here.
sub exhausted {
    Rt::trap('call stack exhausted');
}
