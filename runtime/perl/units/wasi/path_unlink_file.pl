# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
sub wasi_path_unlink_file {
    my ($self, $dirfd, $path_ptr, $path_len) = @_;
    my $rel = $self->{memory}->read_string($path_ptr, $path_len);
    # unlink(2) never follows a trailing symlink: it removes the link.
    my ($host, $err) = $self->resolve_path($dirfd, $rel, 0);
    return $err if defined $err;
    # Perl's unlink refuses directories client-side (EISDIR) before the
    # syscall reports its own flavor; wasmtime surfaces the host unlink(2)
    # errno — EPERM on macOS, EISDIR elsewhere — so probe a
    # directory target and answer it directly.
    my $bare = substr($host, -1) eq '/' ? substr($host, 0, -1) : $host;
    my @st = lstat($bare);
    if (@st && ($st[2] & 0170000) == 0040000) {
        return $^O eq 'darwin' ? ERRNO_PERM : ERRNO_ISDIR;
    }
    unlink($host) or return $self->fs_errno(0 + $!);
    return ERRNO_SUCCESS;
}
