# Rails on `dewasm`

> A Rails blog application whose process holds *no native extension*.

The native extensions it needs are compiled to wasm and converted to Ruby by `dewasm`.
The converted Ruby is loaded in place of the `.so` files.

## Run it

```console
$ mise install && ./examples/apps/scripts/cpython-sdk.sh
$ cd examples/rails
$ bundle install && bin/rails db:setup
$ bin/rails s
```

> [!NOTE]
> `bin/rails db:setup` and `bin/rails s` takes more seconds than in native mode.

Then, the Rails app starts at [`localhost:3000`](http://localhost:3000/).

A user is created via `rails db:seed` (a part of `db:setup`).
The email address and password for this user are as follows:

| Email                | Password |
|----------------------|----------|
| `author@example.com` | `secret` |

You can use these credentials to log in at [`/session/new`](http://localhost:3000/session/new) and create posts at [`/posts/new`](http://localhost:3000/posts/new).

## Features

### ReActionView

Every template goes through [ReActionView](https://github.com/marcoroth/reactionview).
Its engine is [Herb](https://herb-tools.dev), the HTML-aware ERB parser written in C.
Here Herb is **one of the converted gems**.

The `Herb` menu at the top right is Herb's debug mode.
It outlines each element of the page and shows the ERB that produced it.

### Sanitization and Syntax Highlights

A post's body is user-input HTML, and `sanitize` cleans it before it is served.
`sanitize` parses the body with [Nokogiri](https://nokogiri.org/index.html), another converted gem.
Write a post with a `<script>` tag, then turn on `View HTML` on its page: the tag is gone.

`View HTML` shows the syntax highlighted `<pre><code>` block.
The highlighting is done by [`pygments.rb`](https://github.com/pygments/pygments.rb).
That gem spawns a `python3` child process, which runs [Pygments](https://pygments.org).
`dewasm` *removes that Python process too*.
That is, CPython is compiled to wasm and converted to Ruby, like the C extensions.
That CPython runs in the Rails process, and the gem's Ruby and Python are unmodified.

A converted CPython, however, takes more than 30 seconds to boot and import Pygments.
That is too long to pay at every start of the Rails process.
So the initialization happens once, at build time, with [Wizer](https://github.com/bytecodealliance/wizer).
Wizer runs it and writes the memory it leaves into the wasm module.
Now, the converted Ruby starts from an interpreter that is already initialized.
The first highlighting takes about a second, and the next ones a fraction of it.

### Profiles

The `Profiling` button in the page header turns recording on.
Every following request is recorded with [Vernier](https://github.com/jondkinney/vernier), which sees a call into a C method as a frame too.
The page served links to its recording, drawn as a flame graph.
In it, the gems' work appears as `dewasm gem` frames.
No frame is a `native gem` frame: nothing runs in a C extension.

The same application runs on the gems' own native extensions:

```console
$ BUNDLE_WITHOUT=dewasm bundle install
$ BUNDLE_WITHOUT=dewasm bin/rails s
```

Now a recording of the same page shows the same work as `native gem` frames: one C method each.
No frame is a `dewasm gem` frame.

## How it Works

The native extensions are compiled to wasm with [our own `ruby.h`](dewasm_cext/include/ruby.h).
In it, every Ruby C API function is a wasm import.
[A host written in Ruby](dewasm_cext/lib/dewasm_cext.rb) implements those functions.
That is, the C extension works on the objects of the host Ruby process.

The Gemfile asks for a converted gem in one line each:

```ruby
group :dewasm do
  dewasm_gem "sqlite3", "2.10.1"
end
```

`dewasm_gem` fetches the gem and compiles and converts its extension under `bundle install`.
The gems' own Ruby and C code run unmodified.
The whole of it is in [`dewasm_cext/`](dewasm_cext/README.md).
