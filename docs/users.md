# Projects built on dewasm

Downstream projects that ship dewasm-converted code.
Each one is working evidence of the pipeline: a real library compiled to WebAssembly, converted to source, and packaged for a language's own ecosystem.

## dewasm-merman

[dewasm-merman](https://github.com/dewasm/ruby-merman) ([RubyGems](https://rubygems.org/gems/dewasm-merman)) renders Mermaid diagrams in pure Ruby.
It wraps [merman](https://github.com/Latias94/merman), a Rust implementation of Mermaid, compiled to `wasm32-wasip1` and converted with the Ruby backend.
It renders SVG and terminal text across merman's full diagram coverage, and ships a snapshot of the module state taken after initialization, so every render starts from initialized tables at a fraction of the cold cost.

## dewasm-pozeiden

[dewasm-pozeiden](https://github.com/dewasm/ruby-pozeiden) ([RubyGems](https://rubygems.org/gems/dewasm-pozeiden)) also renders Mermaid diagrams in pure Ruby, from [pozeiden](https://github.com/sc2in/pozeiden), a Zig implementation compiled to `wasm32-wasi`.
It is far smaller than dewasm-merman, but covers fewer diagram types, and its license (PolyForm Noncommercial 1.0.0) does not permit commercial use.
