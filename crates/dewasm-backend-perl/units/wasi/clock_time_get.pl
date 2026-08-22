# requires: memory/i64_store
use Time::HiRes ();

sub wasi_clock_time_get {
    my ($self, $id, $precision, $out_ptr) = @_;
    my $ns;
    if ($id == 0) {  # realtime
        $ns = int(Time::HiRes::time() * 1e9);
    } elsif ($id >= 1 && $id <= 3) {  # monotonic / process / thread cputime
        $ns = int(Time::HiRes::clock_gettime(Time::HiRes::CLOCK_MONOTONIC()) * 1e9);
    } else {
        return ERRNO_INVAL;
    }
    $self->{memory}->i64_store($out_ptr, $ns);
    return ERRNO_SUCCESS;
}
