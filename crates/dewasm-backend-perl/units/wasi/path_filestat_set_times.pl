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
    # Core perl exposes no lutimes/utimensat(AT_SYMLINK_NOFOLLOW), so a final-component symlink cannot carry its own times: the utime below follows it (the one NOFOLLOW gap in this backend's WASI surface; the wasi-testsuite expected-failures list carries the attribution).
    Time::HiRes::utime($a, $m, $host) or return $self->fs_errno(0 + $!);
    return ERRNO_SUCCESS;
}
