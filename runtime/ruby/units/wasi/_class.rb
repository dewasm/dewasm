ERRNO_SUCCESS = 0
ERRNO_BADF = 8
ERRNO_INVAL = 28
ERRNO_IO = 29
ERRNO_NOSYS = 52
ERRNO_NOTSUP = 58
ERRNO_SPIPE = 70

attr_reader :memory

def initialize(args: [], env: {})
  @args = args.map(&:to_s)
  @env = env.map { |k, v| "#{k}=#{v}" }
  @fds = { 0 => $stdin, 1 => $stdout, 2 => $stderr }
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
