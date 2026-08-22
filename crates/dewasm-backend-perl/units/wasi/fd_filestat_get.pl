# requires: memory/init, wasi/pack_filestat
use Time::HiRes ();

sub wasi_fd_filestat_get {
    my ($self, $fd, $buf_ptr) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF unless defined $e;
    my @st = $e->{dir} ? Time::HiRes::stat($e->{path}) : Time::HiRes::stat($e->{fh});
    return ERRNO_IO unless @st;
    $self->{memory}->init($buf_ptr, $self->pack_filestat(\@st), 0, 64);
    return ERRNO_SUCCESS;
}
