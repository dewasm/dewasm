# requires: wasi/rights
ERRNO_SUCCESS = 0
ERRNO_BADF = 8
ERRNO_INVAL = 28
ERRNO_IO = 29
ERRNO_NOSYS = 52
ERRNO_NOTSUP = 58
ERRNO_SPIPE = 70
# NOTCAPABLE lives in this always-bundled prelude, not errno_fs, because the per-fd rights model raises it from the stdio-core fd_* units too, not only from the path_* units that pull in errno_fs.
ERRNO_NOTCAPABLE = 76

# A directory descriptor: either a preopen (`preopen_name` set to the guest-visible path passed in `preopens:`) or a directory the guest opened itself via path_open (`preopen_name` nil).
# `entries` is the fd_readdir listing cache, populated lazily.
# Kept in the prelude (rather than with the rest of the filesystem logic) because `initialize` builds one per preopen unconditionally, so it must be available whenever any
# WASI import is used, not only when a filesystem syscall is.
WasiDir = Struct.new(:host_path, :preopen_name, :entries)

attr_reader :memory

def initialize(args: [], env: {}, preopens: {})
  @args = args.map(&:to_s)
  @env = env.map { |k, v| "#{k}=#{v}" }
  @fds = { 0 => $stdin, 1 => $stdout, 2 => $stderr }
  # Per-fd capability metadata: fd => [rights_base, rights_inheriting, fdflags, filetype].
  # `filetype` is what fd_fdstat_get reports, filled in on its first query and nil until then.
  # An open descriptor's filetype cannot change while it is open, and this metadata travels with its fd-table entry (fd_renumber moves both, and fds are never revived after close), so the memoized answer cannot outlive the descriptor it describes.
  # stdio is seeded all-rights (it is never rights-tested and must stay readable/writable); preopens likewise, so a real embedder keeps unrestricted access and path_open derives the narrowed rights from them.
  @fd_meta = {
    0 => [Rt::M64, Rt::M64, 0, nil],
    1 => [Rt::M64, Rt::M64, 0, nil],
    2 => [Rt::M64, Rt::M64, 0, nil],
  }
  # The stdio special-cases (SPIPE on seek/tell/pread/pwrite, no close)
  # key on the objects captured here, in lockstep with the fd table, not on whatever the globals point at when a syscall runs.
  @std_ios = [$stdin, $stdout, $stderr].freeze
  next_fd = 3
  preopens.each do |guest, host|
    # The host path must resolve, but need not be a directory: a single-file preopen (e.g. "/dev/null" for the zeroperl reactor's init probe) is accepted: the guest resolves it as the preopen root itself.
    real = begin
      File.realpath(host)
    rescue SystemCallError => e
      raise ArgumentError, "preopen #{guest.inspect} => #{host.inspect}: #{e.message}"
    end
    @fds[next_fd] = WasiDir.new(real, guest, nil)
    # A preopen is a directory, so its base is the directory-rights set
    # (no FD_WRITE etc.); its inheriting rights carry the full file-rights set so guest-opened files under it get real read/write capability.
    # root_directory() in the testsuite reopens the preopen with exactly these, so seeding all-of-M64 here would wrongly hand a directory the write right and make that reopen fail EISDIR.
    @fd_meta[next_fd] = [DIR_BASE_RIGHTS, DIR_INHERITING_RIGHTS, 0, nil]
    next_fd += 1
  end
  @next_fd = next_fd
  $stdout.binmode
  $stderr.binmode
  $stdin.binmode
end

# Import-provider protocol: a custom WASI runtime replaces this class wholesale by implementing these two methods.
def import(name)
  meth = :"wasi_#{name}"
  respond_to?(meth) ? method(meth) : nil
end

def attach(instance)
  @memory = instance.memory
end
