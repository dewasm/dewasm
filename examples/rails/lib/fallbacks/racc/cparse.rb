# frozen_string_literal: true

# `racc` requires `racc/cparse` (its C extension); on a `LoadError` it uses its Ruby runtime.
# This file, ahead of the gem on the load path, is that `LoadError`.
raise LoadError, "racc/cparse: the Ruby racc runtime is used instead"
