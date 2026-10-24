module PostsHelper
  def highlighted(source, language)
    Pygments.highlight(source, lexer: language, options: { nowrap: true }).html_safe
  end

  def highlight_css
    Rails.cache.fetch("pygments.css") { Pygments.css(".highlight") }
  end
end
