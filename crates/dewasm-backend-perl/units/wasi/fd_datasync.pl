# requires:
# Core perl exposes no fdatasync; IO::Handle::sync (fsync) is the faithful superset (macOS python does the same).
use IO::Handle ();

sub wasi_fd_datasync {
    my ($self, $fd) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    return ERRNO_IO unless defined $e->{fh}->sync;
    return ERRNO_SUCCESS;
}
