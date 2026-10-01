# requires:
# Core Perl has no `fdatasync`; `IO::Handle::sync` (`fsync`) does all it does and more.
# Python on macOS does the same.
use IO::Handle ();

sub wasi_fd_datasync {
    my ($self, $fd) = @_;
    my $e = $self->{fds}{$fd};
    return ERRNO_BADF if !defined($e) || $e->{dir};
    return ERRNO_IO unless defined $e->{fh}->sync;
    return ERRNO_SUCCESS;
}
