# requires: wasi/errno_fs
def within?(base, path)
  path == base || path.start_with?(base + File::SEPARATOR)
end
private :within?

# Resolves a guest-relative path against a directory fd to an absolute
# host path, confined to that directory fd's own (already-realpath'd)
# root. Every call re-validates against its own dirfd's root, so nested
# path_opens can't be used to launder an escape one level cheaper.
#
# Known limitation (ADR-14): this is a check-then-open, not an atomic
# openat(2)-beneath resolution — a TOCTOU race or a symlink planted
# inside the sandbox between the check and the actual filesystem call
# could in principle escape. Accepted for a single-process research/demo
# runtime, not a multi-tenant sandbox host.
def resolve_path(dirfd, rel)
  entry = @fds[dirfd]
  return [nil, ERRNO_BADF] unless entry.is_a?(WasiDir)
  return [nil, ERRNO_PERM] if rel.include?("\0")
  base = entry.host_path
  joined = File.join(base, rel)
  begin
    real = File.realpath(joined)
    return [nil, ERRNO_NOTCAPABLE] unless within?(base, real)
    [real, nil]
  rescue Errno::ENOENT
    begin
      real_parent = File.realpath(File.dirname(joined))
    rescue Errno::ENOENT
      return [nil, ERRNO_NOENT]
    rescue SystemCallError
      return [nil, ERRNO_IO]
    end
    return [nil, ERRNO_NOTCAPABLE] unless within?(base, real_parent)
    [File.join(real_parent, File.basename(joined)), nil]
  rescue Errno::ELOOP
    [nil, ERRNO_LOOP]
  rescue SystemCallError
    [nil, ERRNO_IO]
  end
end
private :resolve_path
