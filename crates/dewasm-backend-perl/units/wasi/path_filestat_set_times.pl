# requires: memory/read_string, wasi/resolve_path, wasi/fst_times, wasi/errno_fs
use Time::HiRes ();

sub wasi_path_filestat_set_times {
    my ($self, $fd, $flags, $path_ptr, $path_len, $atim, $mtim, $fst_flags) = @_;
    my $rel = $self->{memory}->read_string($path_ptr, $path_len);
    my $follow = ($flags & 0x1) != 0;  # lookupflags::SYMLINK_FOLLOW
    my ($host, $err) = $self->resolve_path($fd, $rel, $follow);
    return $err if defined $err;
    my @st = $follow ? Time::HiRes::stat($host) : Time::HiRes::lstat($host);
    return $self->fs_errno(0 + $!) unless @st;
    my ($a, $m, $terr) = $self->fst_times($st[8], $st[9], $atim, $mtim, $fst_flags);
    return $terr if defined $terr;
    # Core Perl has no `lutimes` or `utimensat(AT_SYMLINK_NOFOLLOW)`.
    # So a symbolic link as the final path component cannot carry its own times.
    # The `utime` below follows it.
    # This is the one NOFOLLOW gap in this backend's WASI surface.
    # The expected-failures list for `wasi-testsuite` carries the attribution.
    Time::HiRes::utime($a, $m, $host) or return $self->fs_errno(0 + $!);
    return ERRNO_SUCCESS;
}
