# requires:
use IO::Handle ();

sub wasi_fd_sync {
    my ($self, $fd) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    return ERRNO_IO unless defined $e->{fh}->sync;
    return ERRNO_SUCCESS;
}
