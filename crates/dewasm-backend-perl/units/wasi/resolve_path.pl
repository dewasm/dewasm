# requires: wasi/errno_fs
use Cwd ();
use Errno ();
use File::Basename ();

sub within {
    my ($self, $base, $path) = @_;
    my $prefix = substr($base, -1) eq '/' ? $base : "$base/";
    return $path eq $base || rindex($path, $prefix, 0) == 0;
}

# Lexical escape check: expand "." and ".." on the text alone.
# Then report whether the path climbs out of the base.
# `Cwd::realpath` below is the check that follows symbolic links.
# But it returns `undef` when the escaped-to parent does not exist on disk.
# So a path like "a/../../etc" needs catching here to report NOTCAPABLE rather than NOENT.
# This mirrors the Ruby runtime's File.expand_path guard.
sub lexical_escape {
    my ($self, $rel) = @_;
    my @stack;
    for my $c (split m{/+}, $rel) {
        next if $c eq '' || $c eq '.';
        if ($c eq '..') {
            return 1 unless @stack;
            pop @stack;
        } else {
            push @stack, $c;
        }
    }
    return 0;
}

# Resolves a guest-relative path against a directory `fd` to an absolute host path.
# The result is confined to the root of that directory `fd`, already passed through `realpath`.
# Every call re-validates against the root of its own `dirfd`.
# So nested `path_open` calls can't be used to launder an escape one level cheaper.
#
# A non-directory base `fd` is NOTDIR (a file used as a `dirfd`); a missing one is BADF.
# A leading "/" is NOTCAPABLE before any join (an absolute guest path escapes the preopen).
# A trailing slash is preserved on the returned host path.
# The underlying host call then applies the POSIX "must be a directory" rule.
#
# `$follow_last` false resolves the parent but leaves the final component untouched.
# That is the AT_SYMLINK_NOFOLLOW shape, for system calls that operate on a symbolic link itself.
# Those are `lstat`, `unlink`, `rename`, `rmdir`, `mkdir`, `link`, `symlink`, and `readlink`.
# A trailing "." or ".." is never a symbolic link, so those fall back to full resolution.
#
# Known limitation: this is a check-then-open, not an atomic `openat(2)`-beneath resolution.
# A TOCTOU race could in principle escape.
# So could a symbolic link planted inside the sandbox between the check and the file system call.
# Accepted for a single-process research or example runtime, not a multi-tenant sandbox host.
sub resolve_path {
    my ($self, $dirfd, $rel, $follow_last) = @_;
    $follow_last = 1 unless defined $follow_last;
    my $entry = $self->{fds}{$dirfd};
    return (undef, ERRNO_BADF) unless defined $entry;
    return (undef, ERRNO_NOTDIR) unless $entry->{dir};
    return (undef, ERRNO_INVAL) if index($rel, "\0") >= 0;
    return (undef, ERRNO_NOTCAPABLE) if rindex($rel, '/', 0) == 0;
    my $base = $entry->{path};
    # Containment is checked before existence.
    # A path whose "..s" escape the sandbox is NOTCAPABLE.
    # This holds even when the escaped-to parent does not exist.
    return (undef, ERRNO_NOTCAPABLE) if $self->lexical_escape($rel);
    my $trailing = length($rel) > 1 && substr($rel, -1) eq '/';
    my $suffix = $trailing ? '/' : '';
    (my $core = $rel) =~ s{/+\z}{};
    my $joined = $core eq '' ? $base : "$base/$core";
    my $last = File::Basename::basename($joined);
    if (!$follow_last && $last ne '.' && $last ne '..') {
        my $real_parent = Cwd::realpath(File::Basename::dirname($joined));
        if (!defined $real_parent) {
            my $e = 0 + $!;
            return (undef, ERRNO_LOOP) if $e == Errno::ELOOP();
            return (undef, ERRNO_NOENT) if $e == Errno::ENOENT();
            return (undef, ERRNO_IO);
        }
        return (undef, ERRNO_NOTCAPABLE) unless $self->within($base, $real_parent);
        return ("$real_parent/$last$suffix", undef);
    }
    my $real = Cwd::realpath($joined);
    if (defined $real) {
        return (undef, ERRNO_NOTCAPABLE) unless $self->within($base, $real);
        return ($real . $suffix, undef);
    }
    my $e = 0 + $!;
    return (undef, ERRNO_LOOP) if $e == Errno::ELOOP();
    return (undef, ERRNO_IO) if $e != Errno::ENOENT();
    # The final component is missing (or a dangling symbolic link).
    # So resolve the parent and re-attach it.
    # A create (`path_open` O_CREAT) then still gets a sandboxed target path.
    my $real_parent = Cwd::realpath(File::Basename::dirname($joined));
    if (!defined $real_parent) {
        return (undef, 0 + $! == Errno::ENOENT() ? ERRNO_NOENT : ERRNO_IO);
    }
    return (undef, ERRNO_NOTCAPABLE) unless $self->within($base, $real_parent);
    return ("$real_parent/" . File::Basename::basename($joined) . $suffix, undef);
}
