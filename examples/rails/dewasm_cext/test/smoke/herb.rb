# frozen_string_literal: true

gem_dir = ARGV.fetch(0)
$LOAD_PATH.unshift File.join(gem_dir, "lib")
require "herb"
require "herb/engine"

raise "native extension loaded" if $LOADED_FEATURES.grep(%r{herb/herb\.(so|bundle)$}).any?

template = <<~ERB
  <ul class="posts">
    <% items.each do |item| %>
      <li><%= item %></li>
    <% end %>
  </ul>
ERB

result = Herb.parse(template)
raise "parse errors: #{result.errors.inspect}" unless result.errors.empty?
root = result.value
raise "unexpected AST root #{root.class}" unless root.is_a?(Herb::AST::DocumentNode)
element = root.children.find { |c| c.is_a?(Herb::AST::HTMLElementNode) }
raise "no element parsed" unless element
raise "unexpected tag #{element.tag_name.value}" unless element.tag_name.value == "ul"

broken = Herb.parse("<div><span></div>")
raise "an unclosed tag was not reported" if broken.errors.empty?

tokens = Herb.lex(template).value
raise "lexer gave #{tokens.size} tokens" unless tokens.size > 10

engine = Herb::Engine.new(template, bufvar: "_buf")
items = %w[a <b>]
output = eval(engine.src, binding) # rubocop:disable Security/Eval
expected = "<ul class=\"posts\">\n    <li>a</li>\n    <li><b></li>\n</ul>\n"
unless output.gsub(/\s+/, " ").strip == expected.gsub(/\s+/, " ").strip
  raise "compiled template rendered #{output.inspect}"
end

puts "herb smoke: ok (#{Herb::VERSION}, #{tokens.size} tokens)"
