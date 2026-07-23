# requires: memory/read_string, memory/i32_store, wasi/resolve_path, wasi/errno_fs
def wasi_path_open(dirfd, _dirflags, path_ptr, path_len, oflags, fs_rights_base,
                    _fs_rights_inheriting, fdflags, opened_fd_ptr)
  rel = @memory.read_string(path_ptr, path_len)
  host_path, err = resolve_path(dirfd, rel)
  return err if err

  if oflags & 0x2 != 0 # oflags::DIRECTORY
    return ERRNO_NOTDIR unless File.directory?(host_path)
    @fds[@next_fd] = WasiDir.new(host_path, nil, nil)
  else
    read = fs_rights_base & 0x2 != 0  # rights::FD_READ
    write = fs_rights_base & 0x40 != 0 # rights::FD_WRITE
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
    mode |= File::APPEND if fdflags & 0x1 != 0 # fdflags::APPEND
    io = File.open(host_path, mode, 0o644)
    io.binmode
    @fds[@next_fd] = io
  end
  @memory.i32_store(opened_fd_ptr, @next_fd)
  @next_fd += 1
  ERRNO_SUCCESS
rescue Errno::ENOENT
  ERRNO_NOENT
rescue Errno::EEXIST
  ERRNO_EXIST
rescue Errno::EISDIR
  ERRNO_ISDIR
rescue Errno::ENOTDIR
  ERRNO_NOTDIR
rescue Errno::EACCES
  ERRNO_ACCES
rescue Errno::ELOOP
  ERRNO_LOOP
rescue SystemCallError
  ERRNO_IO
end
