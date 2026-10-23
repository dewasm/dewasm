# frozen_string_literal: true

# Host side of the dewasm `ruby.h` experiment.
# It implements the Ruby C API as imports (module "ruby") over a handle table.
# See include/ruby.h for the VALUE encoding and the import list.
require "monitor"

module DewasmCExt
  # Index n here is `RB_WK(n)` in ruby.h.
  WELL_KNOWN = [
    nil, Object, BasicObject, Kernel, String, Array, Hash, Integer, Float, Symbol, Proc,
    Module, Class, NilClass, TrueClass, FalseClass, IO, Time, Encoding, Comparable, Enumerable,
    Exception, StandardError, RuntimeError, ArgumentError, TypeError, RangeError, IOError,
    NoMemoryError, NotImplementedError, IndexError, KeyError, StopIteration, EncodingError,
    FrozenError, ZeroDivisionError, NameError, NoMethodError, SecurityError, SystemCallError,
    Encoding::CompatibilityError, Range, Regexp, Struct, Numeric, Thread, $stdout, $stderr
  ].freeze

  Qfalse = 0
  Qtrue = 2
  Qnil = 4
  Qundef = 6
  M32 = 0xffff_ffff
  M64 = 0xffff_ffff_ffff_ffff
  TAG_RAISE = 6
  RUBY_DEFAULT_FREE = M32

  T_OBJECT = 0x01
  T_CLASS = 0x02
  T_MODULE = 0x03
  T_FLOAT = 0x04
  T_STRING = 0x05
  T_REGEXP = 0x06
  T_ARRAY = 0x07
  T_HASH = 0x08
  T_STRUCT = 0x09
  T_BIGNUM = 0x0a
  T_FILE = 0x0b
  T_DATA = 0x0c
  T_MATCH = 0x0d
  T_COMPLEX = 0x0e
  T_RATIONAL = 0x0f
  T_NIL = 0x11
  T_TRUE = 0x12
  T_FALSE = 0x13
  T_SYMBOL = 0x14
  T_FIXNUM = 0x15
  T_NAMES = {
    T_OBJECT => "Object", T_CLASS => "Class", T_MODULE => "Module", T_FLOAT => "Float",
    T_STRING => "String", T_REGEXP => "Regexp", T_ARRAY => "Array", T_HASH => "Hash",
    T_STRUCT => "Struct", T_BIGNUM => "Integer", T_FILE => "File", T_DATA => "Data",
    T_MATCH => "MatchData", T_COMPLEX => "Complex", T_RATIONAL => "Rational", T_NIL => "nil",
    T_TRUE => "true", T_FALSE => "false", T_SYMBOL => "Symbol", T_FIXNUM => "Integer"
  }.freeze

  class StaleHandle < StandardError; end

  class Runtime
    include MonitorMixin

    attr_reader :instance, :stats

    # `mod_class` is the dewasm-generated class; `init_name` is the extension's `Init_*` export.
    # `cwd` is the guest's working directory; relative paths the extension opens resolve against it.
    def initialize(mod_class, init_name, gc_threshold: 4096, preopens: {}, cwd: nil)
      super()
      @objs = WELL_KNOWN.dup
      # Object to handle index, by identity; an entry goes when the object does.
      @ids = ObjectSpace::WeakMap.new
      WELL_KNOWN.each_with_index { |o, i| @ids[o] = i if i.positive? }
      # A handle turned weak keeps its index while its object lives somewhere in Ruby.
      # `@weak` maps a slot's key object to the object; `@slot_keys` holds the key objects.
      @weak = ObjectSpace::WeakMap.new
      @slot_keys = []
      @weak_indices = {}
      @perm = {}
      WELL_KNOWN.each_index { |i| @perm[i] = true if i.positive? }
      @live = {}
      @free = []
      @marks = nil
      @registered_addrs = []
      @wrapped = ObjectSpace::WeakMap.new
      @pending_free = []
      @in_frees = false
      @depth = 0
      @blocks = []
      @shadow_stack = []
      @capas = {}.compare_by_identity
      @id_names = {}
      @errinfo = nil
      @method_frames = []
      @encodings = [nil]
      @encoding_ids = {}
      @encoding_names = {}
      @permanent_frame = false
      @gc_threshold = gc_threshold
      @stats = Hash.new(0)
      @rt = mod_class::Rt
      # A module with no WASI import has a constructor without the `preopens:` keyword.
      @inst =
        if mod_class.instance_method(:initialize).parameters.any? { |_, name| name == :preopens }
          mod_class.new({ "ruby" => self }, preopens: preopens)
        else
          mod_class.new({ "ruby" => self })
        end
      @instance = @inst
      @mem = @inst.memory
      @table = @inst.table_export("__indirect_function_table")
      @sp = @inst.global_export("__stack_pointer")
      @inst.invoke("_initialize")
      chdir(cwd) if cwd
      enter(permanent: true) { @inst.invoke(init_name) }
    end

    # Point the guest's C library at `path`.
    # Its `chdir` is an export of every extension built here.
    def chdir(path)
      bytes = path.to_s.b
      p = guest_malloc(bytes.bytesize + 1)
      write_bytes(p, bytes)
      @mem.buffer.set_string("\0", p + bytes.bytesize)
      raise ArgumentError, "chdir(#{path}) failed in the guest" unless @inst.invoke("chdir", p).zero?
    ensure
      guest_free(p) if p
    end

    def import(name)
      m = :"i_#{name}"
      respond_to?(m) ? method(m) : nil
    end

    def attach(_inst); end

    def stack_pointer = @sp.value
    def live_handles = @live.size
    def weak_handles = @weak_indices.size
    def perm_handles = @perm.size

    # Force a handle collection (for tests).
    def collect! = synchronize { collect }

    # ---- VALUE <-> object ------------------------------------------------

    def to_v(obj)
      case obj
      when nil then Qnil
      when true then Qtrue
      when false then Qfalse
      when Integer
        if obj >= -0x4000_0000 && obj <= 0x3fff_ffff
          ((obj << 1) | 1) & M32
        else
          handle(obj)
        end
      else
        handle(obj)
      end
    end

    def from_v(v)
      if v.odd?
        v -= 0x1_0000_0000 if v >= 0x8000_0000
        v >> 1
      else
        case v
        when Qnil then nil
        when Qtrue then true
        when Qfalse then false
        when Qundef then raise StaleHandle, "Qundef crossed into Ruby"
        else
          i = v >> 3
          obj = @objs[i]
          obj = @weak[@slot_keys[i]] if obj.nil? && @weak_indices[i]
          raise StaleHandle, "stale or unknown handle #{v}" if obj.nil?
          obj
        end
      end
    end

    def handle(obj)
      i = @ids[obj]
      if i
        # A weak handle in use again is strong until the next collection.
        if @weak_indices.delete(i)
          @objs[i] = obj
          @live[i] = true
        end
        return i << 3
      end

      i = @free.pop || @objs.size
      @objs[i] = obj
      @slot_keys[i] ||= Object.new
      @ids[obj] = i
      if @permanent_frame || obj.is_a?(Module) || obj.is_a?(Symbol)
        @perm[i] = true
      else
        @live[i] = true
      end
      @stats[:minted] += 1
      i << 3
    end

    def s32(v) = v >= 0x8000_0000 ? v - 0x1_0000_0000 : v
    def s64(v) = v >= 0x8000_0000_0000_0000 ? v - 0x1_0000_0000_0000_0000 : v

    # ---- guest memory ------------------------------------------------------

    def guest_malloc(n)
      p = @inst.invoke("malloc", n)
      raise NoMemoryError, "guest malloc(#{n}) failed" if p.zero?
      p
    end

    def guest_free(p)
      @inst.invoke("free", p) unless p.zero?
    end

    # `Rt::Memory` bundles only the helpers the module itself uses, so the host goes to the buffer.
    def mem_u32(a) = @mem.buffer.get_value(:u32, a)
    def mem_set_u32(a, v) = @mem.buffer.set_value(:u32, a, v & M32)

    def read_bytes(ptr, len) = len.zero? ? "".b : @mem.buffer.get_string(ptr, len).b

    def write_bytes(ptr, bytes)
      @mem.buffer.set_string(bytes, ptr) unless bytes.empty?
    end

    def read_cstr(ptr)
      return nil if ptr.zero?
      buf = @mem.buffer
      limit = buf.size
      chunks = "".b
      pos = ptr
      while pos < limit
        len = [256, limit - pos].min
        chunk = buf.get_string(pos, len)
        if (nul = chunk.index("\0"))
          return (chunks << chunk[0, nul])
        end
        chunks << chunk
        pos += len
      end
      chunks
    end

    def read_values(ptr, n) = Array.new(n) { |i| from_v(mem_u32(ptr + 4 * i)) }

    # ---- frames ------------------------------------------------------------

    # Every entry into guest code goes through here.
    # It saves the shadow stack pointer, so an exception unwinding through guest frames
    # (which run no epilogue) leaves the stack where it was.
    def enter(permanent: false)
      synchronize do
        @depth += 1
        sp = @sp.value
        saved_perm = @permanent_frame
        @permanent_frame = true if permanent
        @shadow_stack.push({})
        begin
          yield
        ensure
          @sp.value = sp
          sync_shadows(@shadow_stack.pop, free: true)
          @permanent_frame = saved_perm
          @depth -= 1
          if @depth.zero?
            run_pending_frees
            collect if @live.size > @gc_threshold
          end
        end
      end
    end

    def call_cfunc(fn, recv_and_args, blk)
      enter do
        @blocks.push(blk)
        begin
          vals = recv_and_args.map { |o| to_v(o) }
          from_v(table_call(fn, type_key(vals.size), *vals))
        ensure
          @blocks.pop
        end
      end
    end

    def call_cfunc_argv(fn, recv, args, blk)
      enter do
        @blocks.push(blk)
        argv = guest_malloc(4 * [args.size, 1].max)
        begin
          args.each_with_index { |a, i| mem_set_u32(argv + 4 * i, to_v(a)) }
          from_v(table_call(fn, :"i32,i32,i32->i32", args.size, argv, to_v(recv)))
        ensure
          guest_free(argv)
          @blocks.pop
        end
      end
    end

    TYPE_KEYS = (0..16).map { |n| :"#{(['i32'] * n).join(',')}->i32" }.freeze
    def type_key(n) = TYPE_KEYS[n]

    # The generated `Rt::Table` bundles only the `callN` arities the module itself uses.
    # So the host reads the slot directly: `[type_symbol, callable]`.
    def table_call(i, key, *args)
      slot = @table.instance_variable_get(:@slots)[i]
      raise @rt::Trap, "uninitialized element #{i}" if slot.nil?
      ty, func = slot
      raise @rt::Trap, "indirect call type mismatch: #{ty} vs #{key}" unless ty == key
      func.call(*args)
    end

    def define(mod, name, fn, argc, kind)
      rt = self
      body =
        case argc
        when -1 then proc { |*args, &blk| rt.with_method(self, name) { rt.call_cfunc_argv(fn, self, args, blk) } }
        when -2 then proc { |*args, &blk| rt.with_method(self, name) { rt.call_cfunc(fn, [self, args], blk) } }
        else
          proc do |*args, &blk|
            unless args.size == argc
              raise ArgumentError, "wrong number of arguments (given #{args.size}, expected #{argc})"
            end
            rt.with_method(self, name) { rt.call_cfunc(fn, [self, *args], blk) }
          end
        end
      case kind
      when :instance then mod.define_method(name, &body)
      when :private, :protected
        mod.define_method(name, &body)
        mod.send(kind, name)
      when :singleton then mod.define_singleton_method(name, &body)
      when :module_function
        mod.define_method(name, &body)
        mod.send(:module_function, name)
      end
    end

    # The method a guest frame runs, for `rb_call_super`.
    def with_method(recv, name)
      @method_frames.push([recv, name])
      yield
    ensure
      @method_frames.pop
    end

    # ---- handle collection -------------------------------------------------

    # Sound at depth 0 only: no guest frame holds a VALUE in a local then.
    # A handle that a registered address or a wrapped struct's `dmark` marks stays strong.
    # Any other handle becomes weak: as in CRuby, a `VALUE` stays valid while its object lives.
    # C code may keep one in memory the host cannot see, such as a libxml2 node's `_private`.
    # Its index is freed once the object is gone.
    def collect
      @stats[:collections] += 1
      @marks = {}
      @registered_addrs.each { |addr| mark(mem_u32(addr)) }
      @wrapped.each_key do |obj|
        cell, type_ptr, dmark = @wrapped[obj]
        dmark = mem_u32(type_ptr + 4) if type_ptr
        next if dmark.nil? || dmark.zero?
        ptr = mem_u32(cell)
        table_call(dmark, :"i32->", ptr) unless ptr.zero?
      end
      @live.keys.each do |i|
        next if @marks[i]
        obj = @objs[i]
        @objs[i] = nil
        @live.delete(i)
        @weak[@slot_keys[i]] = obj
        @weak_indices[i] = true
        @stats[:demoted] += 1
      end
      @weak_indices.keys.each do |i|
        next if @weak.key?(@slot_keys[i])
        @weak_indices.delete(i)
        @free << i
        @stats[:freed] += 1
      end
      @marks = nil
    end

    def mark(v)
      return if v.odd? || v <= Qundef
      @marks[v >> 3] = true if @marks
    end

    def run_pending_frees
      return if @in_frees
      @in_frees = true
      while (item = @pending_free.shift)
        dfree, cell = item
        ptr = mem_u32(cell)
        guest_free(cell)
        next if ptr.zero?
        if dfree == RUBY_DEFAULT_FREE
          guest_free(ptr)
        else
          table_call(dfree, :"i32->", ptr)
        end
        @stats[:dfree] += 1
      end
    ensure
      @in_frees = false
    end

    # ---- string shadows ----------------------------------------------------

    # A guest pointer into a Ruby String is a copy in linear memory.
    # Writes through it are copied back at every sync point.
    def shadow_ptr(v, str)
      @shadow_stack.reverse_each do |frame|
        entry = frame[v]
        return entry[0] if entry
      end
      bytes = str.b
      capa = [@capas[str] || 0, bytes.bytesize].max
      ptr = guest_malloc(capa + 1)
      write_bytes(ptr, bytes)
      @mem.buffer.set_string("\0", ptr + bytes.bytesize)
      @shadow_stack.last[v] = [ptr, bytes, str, capa]
      @stats[:shadows] += 1
      ptr
    end

    def sync_shadows(frame, free: false)
      frame.each_value do |entry|
        ptr, bytes, str, = entry
        cur = read_bytes(ptr, bytes.bytesize)
        if cur != bytes
          str.replace(cur.force_encoding(str.encoding)) unless str.frozen?
          entry[1] = cur
          @stats[:writebacks] += 1
        end
        guest_free(ptr) if free
      end
    end

    def sync_all_shadows = @shadow_stack.each { |f| sync_shadows(f) }

    def drop_shadow(v)
      @shadow_stack.each do |frame|
        next unless (entry = frame.delete(v))
        sync_shadows({ v => entry }, free: true)
      end
    end

    def shadow_entry(v)
      @shadow_stack.reverse_each { |f| return f[v] if f[v] }
      nil
    end

    # ---- imports: numbers -------------------------------------------------

    def i_rb_ll2inum(v) = to_v(s64(v))
    def i_rb_ull2inum(v) = to_v(v)
    def i_rb_num2ll(v)
      n = Integer(from_v(v))
      raise RangeError, "integer #{n} too big to convert to long long" unless n.bit_length < 64
      n & M64
    end
    def i_rb_num2ull(v) = Integer(from_v(v)) & M64
    def i_rb_num2dbl(v) = Float(from_v(v))

    # One 64-bit two's complement word; the codes are CRuby's `rb_integer_pack` results.
    def i_rb_dw_int64_pack(v, out)
      n = Integer(from_v(v))
      @mem.buffer.set_value(:u64, out, n & M64)
      return 0 if n.zero?
      if n.positive?
        n <= M64 ? 1 : 2
      else
        n >= -(1 << 63) ? M32 - 0 : M32 - 1 # -1 and -2 as i32
      end
    end
    def i_rb_float_new(d) = to_v(d)
    def i_rb_to_int(v) = to_v(from_v(v).to_int)
    def i_rb_Integer(v) = to_v(Integer(from_v(v)))
    def i_rb_Float(v) = to_v(Float(from_v(v)))
    def i_rb_String(v) = to_v(String(from_v(v)))
    def i_rb_to_float(v) = to_v(Float(from_v(v)))

    # ---- imports: types ---------------------------------------------------

    def type_of(obj)
      case obj
      when String then T_STRING
      when Array then T_ARRAY
      when Hash then T_HASH
      when Float then T_FLOAT
      when Integer then T_BIGNUM
      when Symbol then T_SYMBOL
      when Class then T_CLASS
      when Module then T_MODULE
      when Regexp then T_REGEXP
      when Struct then T_STRUCT
      when MatchData then T_MATCH
      when Complex then T_COMPLEX
      when Rational then T_RATIONAL
      when IO then T_FILE
      else @wrapped.key?(obj) ? T_DATA : T_OBJECT
      end
    end

    def i_rb_dw_type(v) = type_of(from_v(v))

    def i_rb_check_type(v, t)
      obj = from_v(v)
      actual = v.odd? ? T_FIXNUM : type_of(obj)
      return if actual == t
      raise TypeError, "wrong argument type #{obj.class} (expected #{T_NAMES[t]})"
    end

    def i_rb_obj_class(v) = to_v(from_v(v).class)
    def i_rb_class_of(v) = to_v(from_v(v).class)
    def i_rb_obj_is_kind_of(v, k) = from_v(v).is_a?(from_v(k)) ? Qtrue : Qfalse
    def i_rb_obj_is_instance_of(v, k) = from_v(v).instance_of?(from_v(k)) ? Qtrue : Qfalse
    def i_rb_respond_to(v, id) = from_v(v).respond_to?(from_v(id)) ? 1 : 0
    def i_rb_obj_frozen_p(v) = from_v(v).frozen? ? Qtrue : Qfalse
    def i_rb_obj_freeze(v) = (from_v(v).freeze; v)
    def i_rb_check_frozen(v) = (raise FrozenError, "can't modify frozen #{from_v(v).class}" if from_v(v).frozen?)
    def i_rb_equal(a, b) = from_v(a) == from_v(b) ? Qtrue : Qfalse
    def i_rb_eql(a, b) = from_v(a).eql?(from_v(b)) ? 1 : 0
    def i_rb_inspect(v) = to_v(from_v(v).inspect)
    def i_rb_obj_as_string(v) = to_v(from_v(v).to_s)
    def i_rb_obj_id(v) = to_v(from_v(v).object_id)
    def i_rb_obj_dup(v) = to_v(from_v(v).dup)
    def i_rb_obj_alloc(k) = to_v(from_v(k).allocate)
    def i_rb_class_new_instance(argc, argv, k) = to_v(from_v(k).new(*read_values(argv, argc)))

    def i_rb_class_new_instance_kw(argc, argv, k, kw_splat)
      args = read_values(argv, argc)
      if kw_splat != 0 && args.last.is_a?(Hash)
        kw = args.pop
        to_v(from_v(k).new(*args, **kw))
      else
        to_v(from_v(k).new(*args))
      end
    end

    def i_rb_obj_call_init(obj, argc, argv) = from_v(obj).__send__(:initialize, *read_values(argv, argc))

    def i_rb_obj_call_init_kw(obj, argc, argv, kw_splat)
      args = read_values(argv, argc)
      if kw_splat != 0 && args.last.is_a?(Hash)
        kw = args.pop
        from_v(obj).__send__(:initialize, *args, **kw)
      else
        from_v(obj).__send__(:initialize, *args)
      end
    end

    def i_rb_obj_classname(v) = i_rb_id2name(to_v(from_v(v).class.name.to_sym))
    def i_rb_class2name(k) = i_rb_id2name(to_v(from_v(k).name.to_sym))
    def i_rb_ivar_defined(o, id) = from_v(o).instance_variable_defined?(ivar_name(from_v(id))) ? Qtrue : Qfalse
    def i_rb_proc_arity(p) = from_v(p).arity & M32
    def i_rb_class_new(sup) = to_v(Class.new(from_v(sup)))
    def i_rb_apply(recv, mid, args) = to_v(from_v(recv).__send__(from_v(mid), *from_v(args)))
    def i_rb_str_split(s, p) = to_v(from_v(s).split(read_cstr(p)))

    # `rb_call_super` needs the method the guest is running; the frame stack records it.
    def i_rb_call_super(argc, argv)
      frame = @method_frames.last or raise RuntimeError, "rb_call_super outside a method"
      recv, name = frame
      sup = recv.method(name).super_method or raise NoMethodError, "super: no superclass method '#{name}'"
      to_v(sup.call(*read_values(argv, argc)))
    end

    # `rb_get_kwargs(hash, table, required, optional, values)`: `table` holds IDs, `values` VALUEs.
    # A key in `table` beyond `required` is optional; a missing optional slot is `Qundef`.
    # `optional < 0` allows unknown keys (the `-1 - optional` rest convention of CRuby).
    def i_rb_get_kwargs(hash, table, required, optional, values)
      required = s32(required)
      optional = s32(optional)
      rest = optional.negative?
      optional = -1 - optional if rest
      h = hash == Qnil ? {} : from_v(hash)
      found = 0
      (required + optional).times do |i|
        key = from_v(mem_u32(table + 4 * i))
        if h.key?(key)
          mem_set_u32(values + 4 * i, to_v(h[key])) unless values.zero?
          found += 1
        elsif i < required
          raise ArgumentError, "missing keyword: :#{key}"
        else
          mem_set_u32(values + 4 * i, Qundef) unless values.zero?
        end
      end
      unless rest
        keys = (required + optional).times.map { |i| from_v(mem_u32(table + 4 * i)) }
        unknown = h.keys - keys
        raise ArgumentError, "unknown keyword: :#{unknown.first}" unless unknown.empty?
      end
      found
    end
    def i_rb_class_name(k) = to_v(from_v(k).name)

    # ---- imports: symbols --------------------------------------------------

    def i_rb_intern(p) = to_v(read_cstr(p).force_encoding(Encoding::UTF_8).to_sym)
    def i_rb_intern2(p, n) = to_v(read_bytes(p, n).force_encoding(Encoding::UTF_8).to_sym)
    def i_rb_intern_str(v) = to_v(from_v(v).to_sym)
    def i_rb_sym2str(v) = to_v(from_v(v).to_s)
    def i_rb_str_intern(v) = to_v(from_v(v).to_sym)

    def i_rb_id2name(id)
      sym = from_v(id)
      @id_names[sym] ||= begin
        bytes = sym.to_s.b
        p = guest_malloc(bytes.bytesize + 1)
        write_bytes(p, bytes)
        @mem.buffer.set_string("\0", p + bytes.bytesize)
        p
      end
    end

    # ---- imports: classes, methods -------------------------------------

    def i_rb_define_module(p) = to_v(define_module(Object, read_cstr(p)))
    def i_rb_define_module_under(outer, p) = to_v(define_module(from_v(outer), read_cstr(p)))
    def i_rb_define_class(p, sup) = to_v(define_class(Object, read_cstr(p), from_v(sup)))
    def i_rb_define_class_under(outer, p, sup) = to_v(define_class(from_v(outer), read_cstr(p), from_v(sup)))

    def define_module(outer, name)
      return outer.const_get(name, false) if outer.const_defined?(name, false)
      outer.const_set(name, Module.new)
    end

    def define_class(outer, name, sup)
      return outer.const_get(name, false) if outer.const_defined?(name, false)
      outer.const_set(name, Class.new(sup))
    end

    def i_rb_define_method(k, p, fn, argc) = define(from_v(k), read_cstr(p), fn, s32(argc), :instance)
    def i_rb_define_private_method(k, p, fn, argc) = define(from_v(k), read_cstr(p), fn, s32(argc), :private)
    def i_rb_define_protected_method(k, p, fn, argc) = define(from_v(k), read_cstr(p), fn, s32(argc), :protected)
    def i_rb_define_singleton_method(k, p, fn, argc) = define(from_v(k), read_cstr(p), fn, s32(argc), :singleton)
    def i_rb_define_module_function(k, p, fn, argc) = define(from_v(k), read_cstr(p), fn, s32(argc), :module_function)

    def i_rb_define_alloc_func(k, fn)
      klass = from_v(k)
      rt = self
      klass.define_singleton_method(:allocate) { rt.call_cfunc(fn, [self], nil) }
      klass.define_singleton_method(:new) do |*args, **kw, &blk|
        obj = allocate
        obj.__send__(:initialize, *args, **kw, &blk)
        obj
      end
    end

    def i_rb_undef_alloc_func(k)
      from_v(k).singleton_class.undef_method(:allocate)
    end

    def i_rb_define_const(k, p, v) = from_v(k).const_set(read_cstr(p), from_v(v))
    def i_rb_define_alias(k, a, b) = from_v(k).alias_method(read_cstr(a), read_cstr(b))
    def i_rb_define_attr(k, p, r, w)
      name = read_cstr(p)
      from_v(k).attr_reader(name) if r != 0
      from_v(k).attr_writer(name) if w != 0
    end
    def i_rb_undef_method(k, p) = from_v(k).undef_method(read_cstr(p))
    def i_rb_include_module(k, m) = from_v(k).include(from_v(m))
    def i_rb_extend_object(o, m) = from_v(o).extend(from_v(m))
    def i_rb_const_get(k, id) = to_v(from_v(k).const_get(from_v(id)))
    def i_rb_const_get_at(k, id) = to_v(from_v(k).const_get(from_v(id), false))
    def i_rb_const_defined(k, id) = from_v(k).const_defined?(from_v(id)) ? 1 : 0
    def i_rb_const_set(k, id, v) = from_v(k).const_set(from_v(id), from_v(v))
    def i_rb_path2class(p) = to_v(Object.const_get(read_cstr(p)))
    def i_rb_ivar_get(o, id) = to_v(from_v(o).instance_variable_get(ivar_name(from_v(id))))
    def i_rb_ivar_set(o, id, v) = (from_v(o).instance_variable_set(ivar_name(from_v(id)), from_v(v)); v)
    def i_rb_iv_get(o, p) = to_v(from_v(o).instance_variable_get(ivar_name(read_cstr(p))))
    def i_rb_iv_set(o, p, v) = (from_v(o).instance_variable_set(ivar_name(read_cstr(p)), from_v(v)); v)

    # CRuby lets C code keep an instance variable under a name Ruby code cannot spell, such as
    # `-aggregators`; such a name is mapped onto a spellable one here.
    def ivar_name(name)
      name = name.to_s
      name.start_with?("@") ? name : :"@__dewasm_#{name.gsub(/\W/, '_')}"
    end
    def i_rb_cv_get(k, p) = to_v(from_v(k).class_variable_get(read_cstr(p)))
    def i_rb_cv_set(k, p, v) = from_v(k).class_variable_set(read_cstr(p), from_v(v))
    def i_rb_gv_get(p) = to_v(eval(read_cstr(p).then { |n| n.start_with?("$") ? n : "$#{n}" })) # rubocop:disable Security/Eval
    def i_rb_ext_ractor_safe(_flag) = nil

    # ---- imports: calls and blocks ------------------------------------

    def i_rb_funcallv(recv, mid, argc, argv)
      args = read_values(argv, argc)
      sync_all_shadows
      to_v(from_v(recv).__send__(from_v(mid), *args))
    end

    def i_rb_funcallv_public(recv, mid, argc, argv)
      args = read_values(argv, argc)
      sync_all_shadows
      to_v(from_v(recv).public_send(from_v(mid), *args))
    end

    def i_rb_funcall_with_block(recv, mid, argc, argv, blk)
      args = read_values(argv, argc)
      sync_all_shadows
      to_v(from_v(recv).__send__(from_v(mid), *args, &from_v(blk)))
    end

    def i_rb_block_given_p = @blocks.last ? 1 : 0

    def i_rb_need_block
      raise LocalJumpError, "no block given" unless @blocks.last
    end

    def i_rb_yield(v)
      blk = @blocks.last or raise LocalJumpError, "no block given (yield)"
      sync_all_shadows
      to_v(blk.call(from_v(v)))
    end

    def i_rb_yield_values2(argc, argv)
      blk = @blocks.last or raise LocalJumpError, "no block given (yield)"
      args = read_values(argv, argc)
      sync_all_shadows
      to_v(blk.call(*args))
    end

    def i_rb_yield_splat(ary)
      blk = @blocks.last or raise LocalJumpError, "no block given (yield)"
      sync_all_shadows
      to_v(blk.call(*from_v(ary)))
    end

    def i_rb_block_proc
      blk = @blocks.last or raise ArgumentError, "tried to create Proc object without a block"
      to_v(blk)
    end

    def i_rb_proc_call(pr, args) = to_v(from_v(pr).call(*from_v(args)))

    def i_rb_proc_new(fn, arg)
      rt = self
      to_v(proc { |*a| rt.call_cfunc(fn, [a.first, arg, a.size, a, nil], nil) })
    end

    def i_rb_block_call(recv, mid, argc, argv, fn, data)
      args = read_values(argv, argc)
      rt = self
      to_v(from_v(recv).__send__(from_v(mid), *args) do |*ya|
        rt.call_cfunc(fn, [ya.first, data, ya.size, ya, nil], nil)
      end)
    end

    # ---- imports: exceptions and non-local exits --------------------------

    def i_rb_exc_raise(v) = raise(from_v(v))
    def i_rb_exc_new_str(k, s) = to_v(from_v(k).new(from_v(s)))
    def i_rb_exc_new(k, p, n) = to_v(from_v(k).new(read_bytes(p, n).force_encoding(Encoding::UTF_8)))
    def i_rb_exc_new_cstr(k, p) = to_v(from_v(k).new(read_cstr(p).force_encoding(Encoding::UTF_8)))
    def i_rb_dw_warn_str(s) = warn(from_v(s))
    def i_rb_dw_bug_str(s) = raise(@rt::Trap, "rb_bug: #{from_v(s)}")
    def i_rb_sys_fail(p) = raise(SystemCallError.new(p.zero? ? nil : read_cstr(p), Errno::EINVAL::Errno))
    def i_rb_iter_break = raise(StopIteration)

    def guest_error?(e) = e.is_a?(@rt::Trap) || (@rt.const_defined?(:Exit) && e.is_a?(@rt::Exit))

    def i_rb_protect(fn, arg, state_ptr)
      sp = @sp.value
      begin
        r = table_call(fn, :"i32->i32", arg)
        mem_set_u32(state_ptr, 0)
        r
      rescue Exception => e # rubocop:disable Lint/RescueException
        raise if guest_error?(e)
        @sp.value = sp
        @errinfo = e
        mem_set_u32(state_ptr, TAG_RAISE)
        Qnil
      end
    end

    # A Ruby `ensure` also runs on `break`/`throw`, which no `rescue` sees.
    def i_rb_ensure(b_fn, d1, e_fn, d2)
      sp = @sp.value
      begin
        table_call(b_fn, :"i32->i32", d1)
      ensure
        @sp.value = sp
        table_call(e_fn, :"i32->i32", d2)
      end
    end

    def i_rb_rescue(b_fn, d1, r_fn, d2)
      sp = @sp.value
      begin
        table_call(b_fn, :"i32->i32", d1)
      rescue StandardError => e
        @sp.value = sp
        @errinfo = e
        table_call(r_fn, :"i32,i32->i32", d2, to_v(e))
      end
    end

    def i_rb_jump_tag(_state)
      raise @errinfo || RuntimeError.new("rb_jump_tag without a pending exception")
    end

    def i_rb_errinfo = to_v(@errinfo)
    def i_rb_set_errinfo(v) = (@errinfo = from_v(v))

    # ---- imports: strings ----------------------------------------------

    def i_rb_str_new(p, n) = to_v(p.zero? ? ("\0" * n).b : read_bytes(p, n))
    def i_rb_str_new_cstr(p) = to_v(read_cstr(p))
    def i_rb_utf8_str_new(p, n) = to_v(read_bytes(p, n).force_encoding(Encoding::UTF_8))
    def i_rb_utf8_str_new_cstr(p) = to_v(read_cstr(p).force_encoding(Encoding::UTF_8))
    def i_rb_str_new_frozen(v)
      s = from_v(v)
      to_v(s.frozen? ? s : s.dup.freeze)
    end
    def i_rb_str_dup(v) = to_v(from_v(v).dup)
    def i_rb_str_buf_new(capa)
      s = String.new(capacity: capa)
      @capas[s] = capa
      to_v(s)
    end
    def i_rb_str_cat(v, p, n) = mutate_str(v) { |s| s << read_bytes(p, n) }
    def i_rb_str_cat_cstr(v, p) = mutate_str(v) { |s| s << read_cstr(p) }
    def i_rb_str_append(v, o) = mutate_str(v) { |s| s << from_v(o) }
    def i_rb_str_concat(v, o) = mutate_str(v) { |s| s.concat(from_v(o)) }
    def i_rb_str_plus(a, b) = to_v(from_v(a) + from_v(b))
    def i_rb_str_length(v) = to_v(from_v(v).length)
    def i_rb_str_freeze(v) = (from_v(v).freeze; v)
    def i_rb_str_to_str(v) = i_rb_dw_string_value(v)
    def i_rb_str_equal(a, b) = from_v(a) == from_v(b) ? Qtrue : Qfalse
    def i_rb_str_modify(v) = i_rb_check_frozen(v)

    def i_rb_str_set_len(v, n)
      entry = shadow_entry(v) or return
      ptr, _, str, capa = entry
      raise ArgumentError, "probable buffer overflow: #{n} for #{capa}" if n > capa
      str.replace(read_bytes(ptr, n).force_encoding(str.encoding))
      entry[1] = str.b
    end

    def i_rb_str_resize(v, n)
      drop_shadow(v)
      s = from_v(v)
      if n < s.bytesize
        s.replace(s.byteslice(0, n))
      elsif n > s.bytesize
        s << ("\0" * (n - s.bytesize)).b
      end
      @capas[s] = n
      v
    end

    def mutate_str(v)
      drop_shadow(v)
      yield from_v(v)
      v
    end

    def i_rb_dw_string_value(v)
      obj = from_v(v)
      return v if obj.is_a?(String)
      unless obj.respond_to?(:to_str)
        raise TypeError, "no implicit conversion of #{obj.nil? ? 'nil' : obj.class} into String"
      end
      to_v(obj.to_str)
    end

    def i_rb_check_string_type(v)
      obj = from_v(v)
      obj.is_a?(String) ? v : (obj.respond_to?(:to_str) ? to_v(obj.to_str) : Qnil)
    end

    def i_rb_dw_str_ptr(v) = shadow_ptr(v, from_v(v))
    def i_rb_dw_str_len(v) = from_v(v).bytesize
    def i_rb_obj_encoding(v) = to_v(from_v(v).encoding)

    # ---- imports: encodings ----------------------------------------------
    # An `rb_encoding *` is a non-zero id into `@encodings`; the id doubles as the encoding index.

    def enc_id(enc)
      @encoding_ids[enc] ||= begin
        @encodings << enc
        @encodings.size - 1
      end
    end

    def enc_of(id) = @encodings.fetch(id) { raise ArgumentError, "unknown encoding id #{id}" }

    def i_rb_utf8_encoding = enc_id(Encoding::UTF_8)
    def i_rb_ascii8bit_encoding = enc_id(Encoding::BINARY)
    def i_rb_usascii_encoding = enc_id(Encoding::US_ASCII)
    def i_rb_default_internal_encoding = Encoding.default_internal ? enc_id(Encoding.default_internal) : 0
    def i_rb_default_external_encoding = enc_id(Encoding.default_external)
    def i_rb_locale_encoding = enc_id(Encoding.find("locale"))
    def i_rb_utf8_encindex = enc_id(Encoding::UTF_8)
    def i_rb_ascii8bit_encindex = enc_id(Encoding::BINARY)
    def i_rb_usascii_encindex = enc_id(Encoding::US_ASCII)
    def i_rb_enc_from_index(i) = i
    def i_rb_enc_to_index(e) = e
    def i_rb_enc_find_index(p)
      enc = Encoding.find(read_cstr(p))
      enc ? enc_id(enc) : M32
    rescue ArgumentError
      M32
    end
    def i_rb_enc_find(p) = i_rb_enc_find_index(p).then { |i| i == M32 ? 0 : i }
    def i_rb_enc_get_index(v)
      obj = from_v(v)
      obj.respond_to?(:encoding) ? enc_id(obj.encoding) : 0
    end
    alias i_rb_enc_get i_rb_enc_get_index
    def i_rb_enc_associate(v, e) = (from_v(v).force_encoding(enc_of(e)); v)
    alias i_rb_enc_associate_index i_rb_enc_associate
    def i_rb_enc_from_encoding(e) = e.zero? ? Qnil : to_v(enc_of(e))
    def i_rb_to_encoding(v)
      obj = from_v(v)
      enc_id(obj.is_a?(Encoding) ? obj : Encoding.find(obj.to_str))
    end
    alias i_rb_to_encoding_index i_rb_to_encoding
    def i_rb_enc_name(e)
      @encoding_names[e] ||= begin
        bytes = enc_of(e).name.b
        p = guest_malloc(bytes.bytesize + 1)
        write_bytes(p, bytes)
        @mem.buffer.set_string("\0", p + bytes.bytesize)
        p
      end
    end
    def i_rb_enc_str_new(p, n, e) = to_v(read_bytes(p, n).force_encoding(enc_of(e)))
    def i_rb_enc_str_new_cstr(p, e) = to_v(read_cstr(p).force_encoding(enc_of(e)))
    def i_rb_enc_interned_str(p, n, e) = to_v(-read_bytes(p, n).force_encoding(enc_of(e)))
    def i_rb_enc_interned_str_cstr(p, e) = to_v(-read_cstr(p).force_encoding(enc_of(e)))
    def i_rb_enc_str_buf_cat(v, p, n, e) = mutate_str(v) { |s| s << read_bytes(p, n).force_encoding(enc_of(e)) }
    def i_rb_str_export_to_enc(v, e) = to_v(from_v(v).encode(enc_of(e)))
    def i_rb_str_conv_enc(v, from, to)
      s = from_v(v)
      return v if to.zero?
      to_v(s.encode(enc_of(to), from.zero? ? s.encoding : enc_of(from)))
    rescue EncodingError
      v
    end
    def i_rb_str_encode(v, to, _flags, _opts) = to_v(from_v(v).encode(from_v(to)))
    def i_rb_enc_str_asciionly_p(v) = from_v(v).ascii_only? ? 1 : 0

    # ---- imports: arrays --------------------------------------------------

    def i_rb_ary_new = to_v([])
    def i_rb_ary_new_capa(_n) = to_v([])
    def i_rb_ary_new_from_values(n, p) = to_v(read_values(p, n))
    def i_rb_ary_push(a, v) = (from_v(a).push(from_v(v)); a)
    def i_rb_ary_pop(a) = to_v(from_v(a).pop)
    def i_rb_ary_shift(a) = to_v(from_v(a).shift)
    def i_rb_ary_unshift(a, v) = (from_v(a).unshift(from_v(v)); a)
    def i_rb_ary_entry(a, i) = to_v(from_v(a)[s32(i)])
    def i_rb_ary_store(a, i, v) = (from_v(a)[s32(i)] = from_v(v))
    def i_rb_ary_dup(a) = to_v(from_v(a).dup)
    def i_rb_ary_clear(a) = (from_v(a).clear; a)
    def i_rb_ary_join(a, sep) = to_v(from_v(a).join(from_v(sep)))
    def i_rb_ary_concat(a, b) = (from_v(a).concat(from_v(b)); a)
    def i_rb_ary_includes(a, v) = from_v(a).include?(from_v(v)) ? Qtrue : Qfalse
    def i_rb_ary_delete(a, v) = to_v(from_v(a).delete(from_v(v)))
    def i_rb_ary_reverse(a) = (from_v(a).reverse!; a)
    def i_rb_ary_to_ary(v) = to_v(Array(from_v(v)))
    def i_rb_check_array_type(v)
      obj = from_v(v)
      obj.is_a?(Array) ? v : (obj.respond_to?(:to_ary) ? to_v(obj.to_ary) : Qnil)
    end
    def i_rb_dw_array_len(a) = from_v(a).size

    # ---- imports: hashes --------------------------------------------------

    def i_rb_hash_new = to_v({})
    def i_rb_hash_aref(h, k) = to_v(from_v(h)[from_v(k)])
    def i_rb_hash_aset(h, k, v) = (from_v(h)[from_v(k)] = from_v(v); v)
    def i_rb_hash_lookup(h, k) = to_v(from_v(h).fetch(from_v(k), nil))
    def i_rb_hash_lookup2(h, k, d) = (hh = from_v(h); kk = from_v(k); hh.key?(kk) ? to_v(hh[kk]) : d)
    def i_rb_hash_fetch(h, k) = to_v(from_v(h).fetch(from_v(k)))
    def i_rb_hash_delete(h, k) = to_v(from_v(h).delete(from_v(k)))
    def i_rb_hash_size(h) = to_v(from_v(h).size)
    def i_rb_hash_keys(h) = to_v(from_v(h).keys)
    def i_rb_hash_dup(h) = to_v(from_v(h).dup)
    def i_rb_hash_clear(h) = (from_v(h).clear; h)
    def i_rb_check_hash_type(v)
      obj = from_v(v)
      obj.is_a?(Hash) ? v : (obj.respond_to?(:to_hash) ? to_v(obj.to_hash) : Qnil)
    end
    def i_rb_dw_hash_size_num(h) = from_v(h).size

    def i_rb_hash_foreach(h, fn, arg)
      from_v(h).each do |k, v|
        r = table_call(fn, :"i32,i32,i32->i32", to_v(k), to_v(v), arg)
        break if r == 1
      end
    end

    # ---- imports: typed data ----------------------------------------------

    def i_rb_data_typed_object_wrap(k, ptr, type_ptr)
      wrap(k, ptr, type_ptr, nil, mem_u32(type_ptr + 8))
    end

    def i_rb_data_object_wrap(k, ptr, dmark, dfree)
      wrap(k, ptr, nil, dmark, dfree)
    end

    # An entry is `[cell, type_ptr, dmark]`.
    # The data pointer lives in `cell`, 4 bytes of guest memory, so C code can assign `DATA_PTR`.
    # `ObjectSpace::WeakMap` holds its values weakly too, so the object itself also holds the entry.
    def wrap(k, ptr, type_ptr, dmark, dfree)
      obj = k.zero? ? Object.new : Class.instance_method(:allocate).bind_call(from_v(k))
      cell = guest_malloc(4)
      mem_set_u32(cell, ptr)
      entry = [cell, type_ptr, dmark]
      @wrapped[obj] = entry
      obj.instance_variable_set(:@__dewasm_data, entry)
      ObjectSpace.define_finalizer(obj, finalizer(@pending_free, dfree, cell))
      to_v(obj)
    end

    # Built outside `wrap`, so the `proc`'s binding does not keep the object alive.
    # The cell is read at free time: `DATA_PTR` may have been reassigned since the wrap.
    def finalizer(queue, dfree, cell) = proc { queue << [dfree, cell] }

    def typeddata_match?(entry, type_ptr)
      t = entry[1]
      while t && !t.zero?
        return true if t == type_ptr
        t = mem_u32(t + 24)
      end
      false
    end

    def i_rb_check_typeddata(v, type_ptr)
      obj = from_v(v)
      entry = @wrapped[obj]
      unless entry && typeddata_match?(entry, type_ptr)
        expected = read_cstr(mem_u32(type_ptr))
        raise TypeError, "wrong argument type #{obj.class} (expected #{expected})"
      end
      mem_u32(entry[0])
    end

    def i_rb_typeddata_is_kind_of(v, type_ptr)
      entry = @wrapped[from_v(v)]
      entry && typeddata_match?(entry, type_ptr) ? 1 : 0
    end
    alias i_rb_dw_data_type_p i_rb_typeddata_is_kind_of

    def data_cell(v) = (@wrapped[from_v(v)] or raise TypeError, "not a wrapped object")[0]
    def i_rb_dw_data_ptr(v) = mem_u32(data_cell(v))
    def i_rb_dw_data_set_ptr(v, ptr) = mem_set_u32(data_cell(v), ptr)
    def i_rb_dw_data_ptr_ref(v) = data_cell(v)

    # ---- imports: GC -------------------------------------------------------

    def i_rb_gc_mark(v) = mark(v)
    def i_rb_gc_register_address(addr) = @registered_addrs << addr
    def i_rb_gc_unregister_address(addr) = @registered_addrs.delete(addr)
    def i_rb_gc_register_mark_object(v)
      return if v.odd? || v <= Qundef
      i = v >> 3
      @objs[i] = from_v(v)
      @live.delete(i)
      @weak_indices.delete(i)
      @perm[i] = true
    end
    def i_rb_gc = GC.start

    # ---- imports: other -----------------------------------------------------

    def i_rb_io_write(io, s) = to_v(from_v(io).write(from_v(s)))
    def i_rb_require(p) = (require(read_cstr(p)) ? Qtrue : Qfalse)
    def i_rb_eval_string(p) = to_v(TOPLEVEL_BINDING.eval(read_cstr(p)))
    def i_rb_thread_schedule = Thread.pass
    def i_rb_range_new(a, b, excl) = to_v(Range.new(from_v(a), from_v(b), excl != 0))

    # CRuby's `rb_range_beg_len`: `Qfalse` for a non-Range, `Qnil` when out of `len`, else `Qtrue`
    # with the clipped begin and length stored.
    def i_rb_range_beg_len(v, beg_ptr, len_ptr, len, err)
      range = from_v(v)
      return Qfalse unless range.is_a?(Range)
      len = s32(len)
      b = range.begin.nil? ? 0 : Integer(range.begin)
      e = range.end.nil? ? -1 : Integer(range.end)
      excl = range.end.nil? ? false : range.exclude_end?
      b += len if b.negative?
      e += len if e.negative?
      e += 1 unless excl
      if err.zero? || err == 2
        return Qnil if b > len
        e = len if e > len
      end
      e = b if e < b
      if b.negative? || (err != 0 && (b > len || e > len))
        raise RangeError, "#{range.inspect} out of range" if err != 0
        return Qnil
      end
      mem_set_u32(beg_ptr, b)
      mem_set_u32(len_ptr, e - b)
      Qtrue
    end
    def i_rb_time_new(sec, usec) = to_v(Time.at(s32(sec), s32(usec)))
    def i_rb_num_coerce_bin(a, b, op) = to_v(from_v(a).coerce(from_v(b)).then { |x, y| x.__send__(from_v(op), y) })
  end
end
