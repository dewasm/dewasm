# requires: memory/read_string, memory/iws, wasi/resolve_path, wasi/errno_fs, wasi/rights
def wasi_path_open(dirfd, dirflags, path_ptr, path_len, oflags, base, inheriting, fdflags, opened_fd_ptr)
  rel = @memory.read_string(path_ptr, path_len)
  parent = @fds[dirfd]
  return ERRNO_BADF if parent.nil?
  return ERRNO_NOTDIR unless parent.is_a?(WasiDir)
  return ERRNO_NOTCAPABLE unless fd_has_right?(dirfd, RIGHT_PATH_OPEN)
  # OFLAGS_TRUNC truncates on open; the spec requires the directory's PATH_FILESTAT_SET_SIZE right.
  if oflags & 0x8 != 0 && !fd_has_right?(dirfd, RIGHT_PATH_FILESTAT_SET_SIZE)
    return ERRNO_NOTCAPABLE
  end

  follow = dirflags & 0x1 != 0 # lookupflags::SYMLINK_FOLLOW
  host_path, err = resolve_path(dirfd, rel, follow_last: follow)
  return err if err
  # This unit's branches decide the slash shapes, so strip the preserved slash (issue #42): stat on "file/" fails ENOTDIR and misreads probes.
  trailing = host_path.end_with?("/")
  host_path = host_path.delete_suffix("/")
  # Without SYMLINK_FOLLOW, a symlink at the final component is an error
  # (the caller asked us not to traverse it).
  return ERRNO_LOOP if !follow && File.symlink?(host_path)

  exists = File.exist?(host_path) || File.symlink?(host_path)
  if trailing
    unless exists
      # O_CREAT must not create through the slash; per wasmtime:
      # EINVAL on macOS, EISDIR on Linux, plain open ENOENT on both.
      return ERRNO_NOENT if oflags & 0x1 == 0
      return RUBY_PLATFORM.include?("darwin") ? ERRNO_INVAL : ERRNO_ISDIR
    end
    return ERRNO_NOTDIR unless File.directory?(host_path)
  end

  wants_dir = oflags & 0x2 != 0 # oflags::DIRECTORY
  parent_inheriting = @fd_meta[dirfd][1]

  if wants_dir || (exists && File.directory?(host_path))
    # File.directory? is false for both "missing" and "not a directory";
    # POSIX O_DIRECTORY distinguishes them (ENOENT vs ENOTDIR).
    return ERRNO_NOENT if wants_dir && !exists
    return ERRNO_NOTDIR if wants_dir && !File.directory?(host_path)
    # Opening a directory read/write is EISDIR; only reject when the guest explicitly asked for a directory (a bare open of a dir path just drops the write right below).
    return ERRNO_ISDIR if wants_dir && (base & RIGHT_FD_WRITE != 0)
    @fds[@next_fd] = WasiDir.new(host_path, nil, nil)
    @fd_meta[@next_fd] = [
      base & parent_inheriting & DIR_BASE_RIGHTS,
      inheriting & parent_inheriting & DIR_INHERITING_RIGHTS,
      0,
    ]
  else
    granted = base & parent_inheriting & FILE_BASE_RIGHTS
    read = granted & RIGHT_FD_READ != 0
    write = granted & RIGHT_FD_WRITE != 0
    mode = if read && write
             File::RDWR
           elsif write
             File::WRONLY
           else
             File::RDONLY
           end
    mode |= File::CREAT if oflags & 0x1 != 0 # oflags::CREAT
    mode |= File::EXCL if oflags & 0x4 != 0 # oflags::EXCL
    mode |= File::TRUNC if oflags & 0x8 != 0 # oflags::TRUNC
    io = File.open(host_path, mode, 0o644)
    io.binmode
    @fds[@next_fd] = io
    # APPEND is honored by fd_write (seek-to-end) rather than O_APPEND, so fd_fdstat_set_flags can toggle it; store the whole fdflags word.
    @fd_meta[@next_fd] = [granted, inheriting & parent_inheriting & FILE_BASE_RIGHTS, fdflags & 0x1f]
  end
  @memory.iws(opened_fd_ptr, @next_fd)
  @next_fd += 1
  ERRNO_SUCCESS
rescue SystemCallError => e
  fs_errno(e)
end
