# frozen_string_literal: true

# Pygments, run by a CPython that dewasm converted to Ruby.
#
# `generated/pygments_wasm.rb` is that CPython, built around `ext/pyg.c` and pre-initialized
# by Wizer, so it starts with Pygments imported; `build.rb` makes it under `bundle install`.
# The interpreter reads its standard library and Pygments from two preopened directories, in the
# order Wizer saw them, since the C library remembers each by its file descriptor.
module Dewasm
  module Pygments
    GENERATED = File.expand_path("pygments/generated", __dir__)
    MISSING = "dewasm-pygments is not built; run `bundle install` (or set DEWASM_CEXT_BUILD=1)"

    class Error < StandardError; end

    begin
      require File.join(GENERATED, "pygments_wasm")
    rescue LoadError
      raise LoadError, MISSING
    end

    class << self
      # The HTML of `code` as Pygments marks it up for `language` (a lexer alias such as `html`),
      # with one `<span class>` per token; `nil` for a language Pygments does not know.
      def highlight(code, language)
        code = code.to_s.encode(Encoding::UTF_8)
        language = language.to_s
        call do |instance, memory|
          code_ptr = instance.invoke("malloc", code.bytesize)
          language_ptr = instance.invoke("malloc", language.bytesize)
          memory.buffer.set_string(code, code_ptr, code.bytesize, 0) unless code.empty?
          memory.buffer.set_string(language, language_ptr, language.bytesize, 0)
          pointer = instance.invoke("pyg_highlight", code_ptr, code.bytesize, language_ptr, language.bytesize)
          instance.invoke("free", code_ptr)
          instance.invoke("free", language_ptr)
          result(instance, memory, pointer)
        end
      end

      # `Pygments 2.21.0 on CPython 3.14.6`.
      def version
        call { |instance, memory| result(instance, memory, instance.invoke("pyg_version")) }
      end

      private

      def result(instance, memory, pointer)
        return nil if pointer.zero?

        memory.buffer.get_string(pointer, instance.invoke("pyg_result_length")).force_encoding(Encoding::UTF_8)
      end

      # One interpreter per process, used by one thread at a time.
      def call
        @lock ||= Mutex.new
        @lock.synchronize do
          @instance ||= PygmentsExt.new({}, args: ["python"], env: {},
                                        preopens: { "/lib" => File.join(GENERATED, "lib"),
                                                    "/site" => File.join(GENERATED, "site") })
          yield @instance, @instance.memory
        end
      end
    end
  end
end
