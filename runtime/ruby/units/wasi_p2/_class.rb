# requires: rt/trap
# WASI preview 2 host provider (ADR-21). Methods are *post-lift*: they
# take and return host values (Strings, Arrays, Hashes, Symbols,
# [:case, payload] variants) — ADR-20's synthesized adapters do all the
# pointer work. One resource table holds streams, descriptors, and
# pollables by integer handle; `resource_drop` is called by the
# component wrapper's `canon resource.drop` lambdas.
InStream = Struct.new(:io, :own)
OutStream = Struct.new(:io, :own)
# Filesystem nodes hold a host path, not an open IO: each *-via-stream
# call opens its own handle, so offsets never interfere.
P2Dir = Struct.new(:host_path)
P2Node = Struct.new(:host_path)

def initialize(args: [], env: {}, preopens: {})
  @args = args
  @env = env
  @res = {}
  @next_res = 1
  @preopens = {}
  preopens.each do |guest, host|
    @preopens[guest] = File.realpath(host)
  end
end

# ADR-7 provider protocol over versioned p2 names: resolve
# "wasi:cli/stdout@0.2.9#get-stdout" version-insensitively to
# p2_cli_stdout_get_stdout. Unknown wasi functions bind to a lambda that
# traps at call time, so imports a binary never calls stay linkable.
def import(name)
  iface, func = name.split("#", 2)
  return nil if func.nil? || !iface.start_with?("wasi:")
  m = :"p2_#{(iface.delete_prefix("wasi:").sub(/@[0-9.]+\z/, "") + "_" + func).gsub(/[^a-zA-Z0-9]+/, "_")}"
  return method(m) if respond_to?(m, true)
  ->(*) { Rt.trap("unimplemented WASI p2 function #{name}") }
end

def resource_drop(_id, handle)
  obj = @res.delete(handle)
  obj.io.close if (obj.is_a?(InStream) || obj.is_a?(OutStream)) && obj.own
  nil
end

def res_new(obj)
  h = @next_res
  @next_res += 1
  @res[h] = obj
  h
end

def res(handle) = @res.fetch(handle)

# Sandbox: containment under the preopen's realpath (the ADR-14
# check-then-open model with its accepted TOCTOU caveat).
def p2_resolve(dir, path)
  base = dir.host_path
  full = File.expand_path(path, base)
  return nil unless full == base || full.start_with?("#{base}/")
  parent = File.dirname(full)
  return nil unless File.exist?(parent)
  real_parent = File.realpath(parent)
  return nil unless real_parent == base || real_parent.start_with?("#{base}/")
  File.join(real_parent, File.basename(full))
end
