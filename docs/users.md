# Projects built on `dewasm`

A selected list of projects built on `dewasm`.

## Official

Projects hosted by the `dewasm` team.

### `dewasm-merman` (Ruby)

[`dewasm-merman`](https://github.com/dewasm/ruby-merman) ([RubyGems](https://rubygems.org/gems/dewasm-merman)) renders Mermaid diagrams in pure Ruby.
It wraps [`merman`](https://github.com/Latias94/merman), a Rust implementation of Mermaid.
The gem uses `merman` compiled to `wasm32-wasip1` and converted with the Ruby backend.
It renders SVG and terminal text for every diagram type `merman` covers.

### `dewasm-pozeiden` (Ruby)

[`dewasm-pozeiden`](https://github.com/dewasm/ruby-pozeiden) ([RubyGems](https://rubygems.org/gems/dewasm-pozeiden)) also renders Mermaid diagrams in pure Ruby.
It builds on [`pozeiden`](https://github.com/sc2in/pozeiden), a Zig implementation compiled to `wasm32-wasi`.
It is far smaller than `dewasm-merman`, but covers fewer diagram types.
Its license (PolyForm Noncommercial 1.0.0) does not permit commercial use.

## Unofficial

Unfortunately, there is no such project for now.

If you create or find one, please update this file and open a pull request.
