# requires: memory/read_string, wasi/resolve_path, wasi/errno_fs
def wasi_path_link(old_dirfd, old_flags, old_ptr, old_len, new_dirfd, new_ptr, new_len)
  # `path_link` never dereferences the source symbolic link.
  # A SYMLINK_FOLLOW request is rejected outright.
  return ERRNO_INVAL if old_flags & 0x1 != 0
  old_rel = @memory.read_string(old_ptr, old_len)
  new_rel = @memory.read_string(new_ptr, new_len)
  # A trailing slash on the new name would name a directory, which a hard link can never be.
  return ERRNO_NOENT if new_rel.end_with?("/")
  old_host, err = resolve_path(old_dirfd, old_rel, follow_last: false)
  return err if err
  new_host, err = resolve_path(new_dirfd, new_rel, follow_last: false)
  return err if err
  if linkat
    errno = linkat_nofollow(old_host, new_host)
    raise SystemCallError.new("path_link", errno) unless errno.zero?
  else
    # `link(2)` is all that Ruby offers, and only some platforms make it not follow a symbolic link.
    # Making a hard link to a symbolic link itself is the one request it cannot serve on macOS/BSD.
    # That request is refused rather than silently making a hard link to the target.
    return ERRNO_NOTSUP if RUBY_PLATFORM.include?("darwin") && File.symlink?(old_host)
    File.link(old_host, new_host)
  end
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end

# `linkat(2)` bound through Fiddle, or `false` on a runtime that has no Fiddle.
# Fiddle is the only route from Ruby to `linkat`, and it is not everywhere.
# An ahead-of-time compiled program has no dynamic FFI.
# There `require "fiddle"` may warn and carry on rather than raise.
# So what decides is whether the constant resolves, not whether the `require` succeeded.
def linkat
  return @linkat unless @linkat.nil?
  @linkat = begin
    require "fiddle"
    Fiddle::Function.new(
      Fiddle::Handle::DEFAULT["linkat"],
      [Fiddle::TYPE_INT, Fiddle::TYPE_VOIDP, Fiddle::TYPE_INT, Fiddle::TYPE_VOIDP, Fiddle::TYPE_INT],
      Fiddle::TYPE_INT
    )
  rescue LoadError, NameError, NoMethodError
    false
  end
end
private :linkat

# `link(2)` follows symbolic links on macOS/BSD.
# WASI's `path_link` (without SYMLINK_FOLLOW) must make a hard link to the link itself.
# On every POSIX.1-2008 platform, `linkat(..., 0)` does not follow symbolic links.
# Returns 0 on success or the `errno` on failure.
def linkat_nofollow(old_host, new_host)
  at_fdcwd = RUBY_PLATFORM.include?("darwin") ? -2 : -100
  Fiddle.last_error = 0
  rc = linkat.call(at_fdcwd, old_host, at_fdcwd, new_host, 0)
  rc.zero? ? 0 : Fiddle.last_error
end
private :linkat_nofollow
