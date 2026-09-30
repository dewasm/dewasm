# The real `Kernel#puts` of `mruby-io` (`mrblib/kernel.rb`) is `$stdout.puts`.
# WASI has no `$stdout` IO object here, since `mruby-io` is excluded (see `mrbgem.rake`).
# So this re-implements the same flatten-and-add-a-trailing-newline semantics.
# It sits on top of core `Kernel#print` (`src/print.c`), which needs no gem.
module Kernel
  private def puts(*args)
    if args.empty?
      print("\n")
      return nil
    end
    args.each do |arg|
      if arg.is_a?(Array)
        puts(*arg)
      else
        s = arg.to_s
        print(s)
        print("\n") unless s.end_with?("\n")
      end
    end
    nil
  end
end
