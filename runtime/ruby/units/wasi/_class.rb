ERRNO_SUCCESS = 0
ERRNO_BADF = 8
ERRNO_INVAL = 28
ERRNO_IO = 29
ERRNO_NOSYS = 52
ERRNO_NOTSUP = 58
ERRNO_SPIPE = 70

# A directory descriptor: either a preopen (`preopen_name` set to the
# guest-visible path passed in `preopens:`) or a directory the guest
# opened itself via path_open (`preopen_name` nil). `entries` is the
# fd_readdir listing cache, populated lazily. Kept in the prelude (rather
# than with the rest of the filesystem logic) because `initialize` builds
# one per preopen unconditionally, so it must be available whenever any
# WASI import is used, not only when a filesystem syscall is.
WasiDir = Struct.new(:host_path, :preopen_name, :entries)

attr_reader :memory

def initialize(args: [], env: {}, preopens: {})
  @args = args.map(&:to_s)
  @env = env.map { |k, v| "#{k}=#{v}" }
  @fds = { 0 => $stdin, 1 => $stdout, 2 => $stderr }
  # The stdio special-cases (SPIPE on seek/tell/pread/pwrite, no close)
  # key on the objects captured here, in lockstep with the fd table —
  # not on whatever the globals point at when a syscall runs.
  @std_ios = [$stdin, $stdout, $stderr].freeze
  next_fd = 3
  preopens.each do |guest, host|
    real = begin
      File.realpath(host)
    rescue SystemCallError => e
      raise ArgumentError, "preopen #{guest.inspect} => #{host.inspect}: #{e.message}"
    end
    @fds[next_fd] = WasiDir.new(real, guest, nil)
    next_fd += 1
  end
  @next_fd = next_fd
  $stdout.binmode
  $stderr.binmode
  $stdin.binmode
end

# Import-provider protocol (ADR-7): a custom WASI runtime replaces this
# class wholesale by implementing these two methods.
def import(name)
  meth = :"wasi_#{name}"
  respond_to?(meth) ? method(meth) : nil
end

def attach(instance)
  @memory = instance.memory
end
