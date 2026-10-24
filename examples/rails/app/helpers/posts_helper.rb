module PostsHelper
  # `source` marked up by Pygments, token by token, for a `pre.highlight` block of the template.
  # `pygments.rb` runs Pygments in a Python process; converted, that process is a CPython
  # running as Ruby in this one.
  def highlighted(source, language)
    Pygments.highlight(source, lexer: language, options: { nowrap: true }).html_safe
  end

  # The style rules of Pygments' token classes, for `pre.highlight`; the same for every page.
  def highlight_css
    Rails.cache.fetch("pygments.css") { Pygments.css(".highlight") }
  end
end
