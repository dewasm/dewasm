# requires: rt/trap
# The call-depth limit: generated functions check `$Rt::DEPTH` against `$Rt::LIMIT` and land here.
sub exhausted {
    Rt::trap('call stack exhausted');
}
