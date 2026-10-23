# frozen_string_literal: true

# The unmodified `nokogiri` gem on the converted extension: `libxml2`, `libxslt`, and `gumbo`.
# Usage: `ruby nokogiri.rb <unpacked gem dir>`.
gem_dir = ARGV.fetch(0)
$LOAD_PATH.unshift File.join(gem_dir, "lib")
require "nokogiri"

raise "native extension loaded" if $LOADED_FEATURES.grep(%r{nokogiri/nokogiri\.(so|bundle)$}).any?

def check(desc)
  yield
  puts "ok   #{desc}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "FAIL #{desc}: #{e.class}: #{e.message}"
  puts e.backtrace.first(8).map { |l| "     #{l}" }
  $failed = true
end

check("version info names the packaged libraries") do
  info = Nokogiri::VERSION_INFO
  raise info.inspect unless info["libxml"]["loaded"] == "2.13.9" && info["libxslt"]["loaded"] == "1.1.43"
  raise info.inspect unless info["libxml"]["source"] == "packaged"
end

html = <<~HTML
  <!DOCTYPE html>
  <html><head><title>Hello</title></head>
  <body>
    <h1 id="top" class="title">Hi <em>there</em></h1>
    <ul><li>one</li><li class="x">two</li><li>三</li></ul>
    <p>unclosed <b>bold
    <script>alert(1)</script>
  </body></html>
HTML

check("HTML5 (gumbo): parse, CSS, text, serialize") do
  doc = Nokogiri::HTML5(html)
  raise doc.class.name unless doc.is_a?(Nokogiri::HTML5::Document)
  raise "title" unless doc.title == "Hello"
  raise "css" unless doc.css("li.x").map(&:text) == ["two"]
  raise "css 2" unless doc.css("ul li").size == 3
  raise "unicode" unless doc.css("li").last.text == "三"
  raise "xpath" unless doc.xpath("//h1/@id").first.value == "top"
  raise "recovery" unless doc.at_css("b").text.strip.start_with?("bold")
  out = doc.to_html
  raise "serialize" unless out.include?("<li class=\"x\">two</li>") && out.include?("三")
end

check("HTML4 (libxml2): parse and search") do
  doc = Nokogiri::HTML4(html)
  raise doc.class.name unless doc.is_a?(Nokogiri::HTML4::Document)
  raise "css" unless doc.css("h1 em").first.text == "there"
  raise "errors" unless doc.errors.is_a?(Array)
end

check("XML: namespaces, XPath, mutation, serialization") do
  doc = Nokogiri::XML(<<~XML)
    <root xmlns:a="urn:a"><a:item n="1">x</a:item><a:item n="2">y</a:item></root>
  XML
  items = doc.xpath("//a:item", "a" => "urn:a")
  raise "ns xpath" unless items.map { |i| i["n"] } == %w[1 2]
  items.first.content = "changed"
  items.last.remove
  node = Nokogiri::XML::Node.new("new", doc)
  node["k"] = "v"
  doc.root.add_child(node)
  xml = doc.to_xml
  raise xml unless xml.include?("<a:item n=\"1\">changed</a:item>") && xml.include?("<new k=\"v\"/>")
  raise "removed" if xml.include?("n=\"2\"")
end

check("XML syntax errors are reported, and strict mode raises") do
  doc = Nokogiri::XML("<root><unclosed></root>")
  raise "no error" if doc.errors.empty?
  raise doc.errors.first.class.name unless doc.errors.first.is_a?(Nokogiri::XML::SyntaxError)
  begin
    Nokogiri::XML("<root><unclosed></root>", &:strict)
    raise "strict did not raise"
  rescue Nokogiri::XML::SyntaxError
  end
end

check("builder") do
  built = Nokogiri::HTML4::Builder.new do |b|
    b.div(class: "c") do
      b.span "text & <more>"
      b.br
    end
  end
  raise built.to_html unless built.to_html.include?("<span>text &amp; &lt;more&gt;</span>")
end

check("XSLT transform") do
  xslt = Nokogiri::XSLT(<<~XSL)
    <xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
      <xsl:output method="html"/>
      <xsl:template match="/"><ul><xsl:for-each select="//i"><li><xsl:value-of select="."/></li></xsl:for-each></ul></xsl:template>
    </xsl:stylesheet>
  XSL
  out = xslt.transform(Nokogiri::XML("<r><i>a</i><i>b</i></r>")).to_s
  raise out unless out.include?("<li>a</li>") && out.include?("<li>b</li>")
end

check("fragments and encodings") do
  frag = Nokogiri::HTML5.fragment("<p>日本語 <b>太字</b></p>")
  raise frag.to_html unless frag.to_html == "<p>日本語 <b>太字</b></p>"
  raise "encoding" unless frag.to_html.encoding == Encoding::UTF_8
  doc = Nokogiri::HTML4("<p>caf\xE9</p>".b, nil, "ISO-8859-1")
  raise doc.at_css("p").text.inspect unless doc.at_css("p").text == "café"
end

check("many documents: wrappers are released") do
  200.times { |i| Nokogiri::HTML5("<p>#{i}</p>").css("p").first.text }
  GC.start
  Nokogiri::HTML5("<p>last</p>")
end

check("SAX parser callbacks reach Ruby") do
  seen = []
  handler = Class.new(Nokogiri::XML::SAX::Document) do
    define_method(:start_element) { |name, attrs| seen << name }
  end.new
  Nokogiri::XML::SAX::Parser.new(handler).parse("<a><b/><c/></a>")
  raise seen.inspect unless seen == %w[a b c]
end

if $failed
  puts "nokogiri smoke: FAILED"
  exit 1
end
puts "nokogiri smoke: ok"
