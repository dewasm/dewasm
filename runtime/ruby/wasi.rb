# dewasmify WASI preview 1 runtime for Ruby.
#
# Implements the core syscalls used by typical command-line programs
# (stdio, args, environ, clocks, random, proc_exit). Filesystem access via
# path_open and preopens is not implemented yet; unimplemented syscalls
# return ENOSYS.

require "securerandom"

module Dewasmify
  class WASI
    ERRNO_SUCCESS = 0
    ERRNO_BADF = 8
    ERRNO_INVAL = 28
    ERRNO_IO = 29
    ERRNO_NOSYS = 52
    ERRNO_NOTSUP = 58
    ERRNO_SPIPE = 70

    FUNCTIONS = %w[
      args_get args_sizes_get environ_get environ_sizes_get
      clock_res_get clock_time_get
      fd_advise fd_allocate fd_close fd_datasync fd_fdstat_get
      fd_fdstat_set_flags fd_fdstat_set_rights fd_filestat_get
      fd_filestat_set_size fd_filestat_set_times fd_pread fd_prestat_get
      fd_prestat_dir_name fd_pwrite fd_read fd_readdir fd_renumber
      fd_seek fd_sync fd_tell fd_write
      path_create_directory path_filestat_get path_filestat_set_times
      path_link path_open path_readlink path_remove_directory path_rename
      path_symlink path_unlink_file
      poll_oneoff proc_exit proc_raise random_get sched_yield
      sock_accept sock_recv sock_send sock_shutdown
    ].freeze

    attr_accessor :memory

    def initialize(args: [], env: {})
      @args = args.map(&:to_s)
      @env = env.map { |k, v| "#{k}=#{v}" }
      @fds = { 0 => $stdin, 1 => $stdout, 2 => $stderr }
      $stdout.binmode
      $stderr.binmode
      $stdin.binmode
    end

    # Returns the imports hash to pass to a generated module's constructor.
    # Set `wasi.memory = instance.memory` right after instantiation.
    def imports
      table = {}
      FUNCTIONS.each do |name|
        table[name] =
          if respond_to?("wasi_#{name}")
            method("wasi_#{name}")
          else
            ->(*_args) { ERRNO_NOSYS }
          end
      end
      { "wasi_snapshot_preview1" => table }
    end

    def wasi_proc_exit(code)
      raise Exit, code
    end

    def wasi_args_sizes_get(argc_ptr, buf_size_ptr)
      @memory.i32_store(argc_ptr, @args.size)
      @memory.i32_store(buf_size_ptr, @args.sum { |a| a.bytesize + 1 })
      ERRNO_SUCCESS
    end

    def wasi_args_get(argv_ptr, buf_ptr)
      write_string_list(@args, argv_ptr, buf_ptr)
    end

    def wasi_environ_sizes_get(count_ptr, buf_size_ptr)
      @memory.i32_store(count_ptr, @env.size)
      @memory.i32_store(buf_size_ptr, @env.sum { |e| e.bytesize + 1 })
      ERRNO_SUCCESS
    end

    def wasi_environ_get(environ_ptr, buf_ptr)
      write_string_list(@env, environ_ptr, buf_ptr)
    end

    def wasi_clock_res_get(_id, out_ptr)
      @memory.i64_store(out_ptr, 1)
      ERRNO_SUCCESS
    end

    def wasi_clock_time_get(id, _precision, out_ptr)
      ns =
        case id
        when 0 then Process.clock_gettime(Process::CLOCK_REALTIME, :nanosecond)
        when 1, 2, 3 then Process.clock_gettime(Process::CLOCK_MONOTONIC, :nanosecond)
        else return ERRNO_INVAL
        end
      @memory.i64_store(out_ptr, ns & M64)
      ERRNO_SUCCESS
    end

    def wasi_random_get(buf_ptr, len)
      @memory.init(buf_ptr, SecureRandom.bytes(len), 0, len)
      ERRNO_SUCCESS
    end

    def wasi_fd_write(fd, iovs_ptr, iovs_len, nwritten_ptr)
      io = @fds[fd]
      return ERRNO_BADF unless io
      written = 0
      iovs_len.times do |i|
        ptr = @memory.i32_load(iovs_ptr + i * 8)
        len = @memory.i32_load(iovs_ptr + i * 8 + 4)
        written += io.write(@memory.read_string(ptr, len))
      end
      io.flush
      @memory.i32_store(nwritten_ptr, written)
      ERRNO_SUCCESS
    rescue SystemCallError
      ERRNO_IO
    end

    def wasi_fd_read(fd, iovs_ptr, iovs_len, nread_ptr)
      io = @fds[fd]
      return ERRNO_BADF unless io
      nread = 0
      iovs_len.times do |i|
        ptr = @memory.i32_load(iovs_ptr + i * 8)
        len = @memory.i32_load(iovs_ptr + i * 8 + 4)
        next if len == 0
        chunk = io.read(len)
        break if chunk.nil?
        @memory.init(ptr, chunk, 0, chunk.bytesize)
        nread += chunk.bytesize
        break if chunk.bytesize < len
      end
      @memory.i32_store(nread_ptr, nread)
      ERRNO_SUCCESS
    rescue SystemCallError
      ERRNO_IO
    end

    def wasi_fd_close(fd)
      io = @fds.delete(fd)
      return ERRNO_BADF unless io
      io.close unless [$stdin, $stdout, $stderr].include?(io)
      ERRNO_SUCCESS
    end

    def wasi_fd_fdstat_get(fd, out_ptr)
      io = @fds[fd]
      return ERRNO_BADF unless io
      filetype = io.respond_to?(:tty?) && io.tty? ? 2 : 4 # char device / regular file
      @memory.fill(out_ptr, 0, 24)
      @memory.i32_store8(out_ptr, filetype)
      @memory.i64_store(out_ptr + 8, M64)  # rights base: everything
      @memory.i64_store(out_ptr + 16, M64) # rights inheriting: everything
      ERRNO_SUCCESS
    end

    def wasi_fd_seek(fd, offset, whence, out_ptr)
      io = @fds[fd]
      return ERRNO_BADF unless io
      return ERRNO_SPIPE if [$stdin, $stdout, $stderr].include?(io)
      mode = [IO::SEEK_SET, IO::SEEK_CUR, IO::SEEK_END][whence]
      return ERRNO_INVAL unless mode
      io.seek(Dewasmify.s64(offset), mode)
      @memory.i64_store(out_ptr, io.tell & M64)
      ERRNO_SUCCESS
    rescue SystemCallError
      ERRNO_IO
    end

    def wasi_fd_tell(fd, out_ptr)
      io = @fds[fd]
      return ERRNO_BADF unless io
      @memory.i64_store(out_ptr, io.tell & M64)
      ERRNO_SUCCESS
    rescue SystemCallError
      ERRNO_IO
    end

    def wasi_fd_prestat_get(_fd, _out_ptr)
      ERRNO_BADF # no preopened directories yet
    end

    def wasi_sched_yield
      ERRNO_SUCCESS
    end

    private

    def write_string_list(strings, list_ptr, buf_ptr)
      strings.each_with_index do |s, i|
        @memory.i32_store(list_ptr + i * 4, buf_ptr)
        @memory.init(buf_ptr, s, 0, s.bytesize)
        @memory.i32_store8(buf_ptr + s.bytesize, 0)
        buf_ptr += s.bytesize + 1
      end
      ERRNO_SUCCESS
    end
  end
end
