module PostsHelper
  # `source` marked up by Pygments, token by token, as a `<pre>` block.
  # `pygments.rb` runs Pygments in a Python process; converted, that process is a CPython
  # running as Ruby in this one.
  def highlighted(source, language)
    html = Pygments.highlight(source, lexer: language, options: { nowrap: true })
    tag.pre(class: "highlight") { tag.code(html.html_safe) }
  end

  # The style rules of Pygments' token classes, for `pre.highlight`; the same for every page.
  def highlight_css
    Rails.cache.fetch("pygments.css") { Pygments.css(".highlight") }
  end

  # Where Pygments runs, for the page: on the converted CPython, or in a Python process.
  # The converted gem's loader is what defines `Popen.dewasm_call`.
  def pygments_description
    converted = Pygments::Popen.respond_to?(:dewasm_call)
    runtime = converted ? "a CPython converted to Ruby" : "a Python process"
    "Pygments #{Pygments.pygments_version} through pygments.rb, on #{runtime}"
  end
end
