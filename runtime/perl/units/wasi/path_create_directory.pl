# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
sub wasi_path_create_directory {
    my ($self, $dirfd, $path_ptr, $path_len) = @_;
    my $rel = $self->{memory}->read_string($path_ptr, $path_len);
    # Strip a trailing slash: mkdir names a directory anyway, and EEXIST is
    # wasmtime's answer for mkdir("file/") where the hosts split.
    (my $core = $rel) =~ s{/+\z}{};
    $rel = $core if $core ne '';
    # mkdir(2) never follows a trailing symlink (an existing one is EEXIST).
    my ($host, $err) = $self->resolve_path($dirfd, $rel, 0);
    return $err if defined $err;
    mkdir($host) or return $self->fs_errno(0 + $!);
    return ERRNO_SUCCESS;
}
