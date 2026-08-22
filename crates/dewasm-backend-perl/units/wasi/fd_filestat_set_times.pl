# requires: wasi/fst_times
use Time::HiRes ();

sub wasi_fd_filestat_set_times {
    my ($self, $fd, $atim, $mtim, $fst_flags) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    my @st = Time::HiRes::stat($e->{fh});
    return ERRNO_IO unless @st;
    my ($a, $m, $err) = $self->fst_times($st[8], $st[9], $atim, $mtim, $fst_flags);
    return $err if defined $err;
    Time::HiRes::utime($a, $m, $e->{fh}) or return ERRNO_IO;
    return ERRNO_SUCCESS;
}
