module PostsHelper
  # `source` marked up by Pygments, token by token, as a `<pre>` block.
  # Pygments runs on a CPython that dewasm converted to Ruby; each call is one request to it.
  def highlighted(source, language)
    html = Dewasm::Pygments.highlight(source, language)
    tag.pre(class: "highlight") { tag.code(html ? html.html_safe : source) }
  end
end
