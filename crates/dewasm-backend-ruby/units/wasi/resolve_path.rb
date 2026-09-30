# requires: wasi/errno_fs
def within?(base, path)
  prefix = base.end_with?(File::SEPARATOR) ? base : base + File::SEPARATOR
  path == base || path.start_with?(prefix)
end
private :within?

# Resolves a guest-relative path against a directory descriptor to an absolute host path.
# The result is confined to that directory descriptor's own root, already resolved by `realpath`.
# Every call re-validates against its own `dirfd`'s root.
# So nested `path_open` calls can't be used to launder an escape one level cheaper.
#
# A trailing slash is stripped for resolution and re-appended to the returned host path.
# So the host call checks it (issue #42).
# This mirrors `crates/dewasm-backend-python/units/wasi/resolve_path.py`.
#
# `follow_last: false` resolves the parent but leaves the final component untouched.
# That is the AT_SYMLINK_NOFOLLOW shape.
# It serves system calls that operate on a symbolic link itself.
# Those are `lstat`, `unlink`, `rename`, `rmdir`, `mkdir`, `link`, `symlink`, `readlink`.
# A trailing "." or ".." is never a symbolic link, so those fall back to full resolution.
#
# Known limitation: this is a check-then-open, not an atomic `openat(2)`-beneath resolution.
# A TOCTOU race could in principle escape.
# So could a symbolic link planted inside the sandbox between the check and the file system call.
# Accepted for a single-process research and example runtime, not a multi-tenant sandbox host.
def resolve_path(dirfd, rel, follow_last: true)
  entry = @fds[dirfd]
  return [nil, ERRNO_BADF] if entry.nil?
  return [nil, ERRNO_NOTDIR] unless entry.is_a?(WasiDir)
  return [nil, ERRNO_INVAL] if rel.include?("\0")
  # An absolute guest path is an escape attempt.
  # File.join would silently collapse the leading slash back inside the base, so reject it outright.
  return [nil, ERRNO_NOTCAPABLE] if rel.start_with?("/")
  base = entry.host_path
  # Lexical escape guard: expand ".." against the base on the text alone.
  # Reject anything that climbs out.
  # `realpath` below is the check that resolves symbolic links.
  # It can't run when the escaped parent doesn't exist on disk (it would raise ENOENT).
  # So a path like "a/../../etc" needs catching here to report NOTCAPABLE rather than NOENT.
  return [nil, ERRNO_NOTCAPABLE] unless within?(base, File.expand_path(rel, base))
  trailing = rel.length > 1 && rel.end_with?("/")
  suffix = trailing ? "/" : ""
  core = rel.sub(%r{/+\z}, "")
  joined = core.empty? ? base : File.join(base, core)
  last = File.basename(joined)
  if !follow_last && last != "." && last != ".."
    begin
      real_parent = File.realpath(File.dirname(joined))
    rescue Errno::ENOENT
      return [nil, ERRNO_NOENT]
    rescue Errno::ELOOP
      return [nil, ERRNO_LOOP]
    rescue SystemCallError
      return [nil, ERRNO_IO]
    end
    return [nil, ERRNO_NOTCAPABLE] unless within?(base, real_parent)
    return [File.join(real_parent, last) + suffix, nil]
  end
  begin
    real = File.realpath(joined)
    return [nil, ERRNO_NOTCAPABLE] unless within?(base, real)
    [real + suffix, nil]
  rescue Errno::ENOENT
    begin
      real_parent = File.realpath(File.dirname(joined))
    rescue Errno::ENOENT
      return [nil, ERRNO_NOENT]
    rescue SystemCallError
      return [nil, ERRNO_IO]
    end
    return [nil, ERRNO_NOTCAPABLE] unless within?(base, real_parent)
    [File.join(real_parent, File.basename(joined)) + suffix, nil]
  rescue Errno::ELOOP
    [nil, ERRNO_LOOP]
  rescue SystemCallError
    [nil, ERRNO_IO]
  end
end
private :resolve_path
