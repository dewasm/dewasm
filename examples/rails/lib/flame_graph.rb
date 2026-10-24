# frozen_string_literal: true

require "json"

class FlameGraph
  KINDS = {
    "Ruby (app, Rails, gems)" => "#e9ecef",
    "dewasm gem" => "#3b82f6",
    "native gem" => "#f97316",
    "dewasm cext" => "#a8c5fa",
    "cfunc (Ruby core)" => "#cfd4da"
  }.freeze
  NATIVE_NAMESPACES = {
    "SQLite3" => "sqlite3", "Nokogiri" => "nokogiri", "Herb" => "herb", "BCrypt" => "bcrypt"
  }.freeze
  ROW = 16
  MIN_WIDTH = 0.4

  Node = Struct.new(:func, :weight, :children)

  attr_reader :total, :root, :func_names, :func_files, :self_weight

  def self.from_vernier(profile)
    profile = JSON.parse(profile) if profile.is_a?(String)
    thread = profile["threads"].max_by { |t| t["samples"]["length"] }
    strings = thread["stringArray"] || profile.dig("shared", "stringArray")
    names = thread["funcTable"]["name"].map { |i| strings[i] }
    files = thread["funcTable"]["fileName"].map { |i| i && strings[i] }
    frame_func = thread["frameTable"]["func"]
    stack_frame = thread["stackTable"]["frame"]
    stack_prefix = thread["stackTable"]["prefix"]
    samples = thread["samples"]
    weights = samples["weight"] || Array.new(samples["length"], 1)
    resolved = {}
    stacks = samples["stack"].each_with_index.filter_map do |stack, i|
      next if stack.nil?
      chain = resolved[stack] ||= begin
        chain = []
        s = stack
        while s
          chain << frame_func[stack_frame[s]]
          s = stack_prefix[s]
        end
        chain.reverse!
      end
      [chain, weights[i]]
    end
    new(names, files, stacks)
  end

  def initialize(func_names, func_files, stacks)
    @func_names = func_names
    @func_files = func_files
    @kinds, @gems = func_files.zip(func_names).map { |f, n| self.class.classify(f, n) }.transpose
    @root = Node.new(nil, 0, {})
    @self_weight = Hash.new(0)
    @kind_weight = Hash.new(0)
    @kinds_seen = {}
    @extension_on_stack = Hash.new(0)
    @total = 0
    stacks.each do |chain, w|
      @total += w
      node = @root
      node.weight += w
      chain.each do |f|
        node = (node.children[f] ||= Node.new(f, 0, {}))
        node.weight += w
        @kinds_seen[@kinds[f]] = true
      end
      leaf = chain.last
      @self_weight[leaf] += w
      @kind_weight[@kinds[leaf]] += w
      chain.filter_map { |f| extension_label(f) }.uniq.each { |label| @extension_on_stack[label] += w }
    end
  end

  def self.classify(file, name = "")
    case file
    when nil, /\A<cfunc>/
      gem = NATIVE_NAMESPACES[name[/\A([A-Z]\w*)(?:::|[#.])/, 1]]
      gem ? ["native gem", gem] : ["cfunc (Ruby core)", nil]
    when %r{/([a-z0-9_]+)_wasm\.rb\z}
      ["dewasm gem", Regexp.last_match(1).sub(/_(ext|native)\z/, "")]
    when %r{/dewasm_cext\.rb\z} then ["dewasm cext", nil]
    else ["Ruby (app, Rails, gems)", nil]
    end
  end

  def self.color_of(kind) = KINDS.fetch(kind)

  def native? = @kinds_seen.key?("native gem")

  def legend = KINDS.keys.select { |kind| @kinds_seen[kind] }

  def display_name(func) = @func_names[func].sub(/#_f\d+_(?=\w)/, "#")

  def kind(func) = @kinds[func]

  def gem_of(func) = @gems[func]

  def label(func) = @gems[func] ? "#{@kinds[func]}: #{@gems[func]}" : @kinds[func]

  def extension_label(func)
    @gems[func] && "#{@kinds[func].delete_suffix(' gem')} #{@gems[func]}"
  end

  def pct(w) = @total.zero? ? "0.0%" : format("%.1f%%", 100.0 * w / @total)

  def by_kind = @kind_weight.sort_by { |_, w| -w }

  def extensions_on_stack = @extension_on_stack.sort_by { |_, w| -w }

  def top_self(n = 15) = @self_weight.sort_by { |_, w| -w }.first(n)

  def depth(node = @root, d = 0)
    node.children.values.map { |child| depth(child, d + 1) }.max || d
  end

  # A focus path names a child by its position in this order, so the order has to be stable.
  def ordered_children(node) = node.children.values.sort_by { |c| @func_names[c.func] }

  def resolve_focus(path)
    node = @root
    ancestors = []
    (path || "").split(".").each do |i|
      child = ordered_children(node)[i.to_i] or break
      ancestors << child
      node = child
    end
    [node, ancestors]
  end

  def svg(width: 1400, focus: nil, title: nil, link: nil)
    focus_node, ancestors = resolve_focus(focus)
    base = ancestors.empty? ? 0 : 1
    max_depth = depth(focus_node) + base
    height = (max_depth + 3) * ROW + 44
    out = +""
    out << %(<svg xmlns="http://www.w3.org/2000/svg" width="#{width}" height="#{height}" )
    out << %(viewBox="0 0 #{width} #{height}" font-family="ui-monospace, Menlo, monospace" font-size="11">\n)
    out << %(<style>rect:hover { stroke: #000; stroke-width: 1; } text { pointer-events: none; }</style>\n)
    out << %(<rect width="100%" height="100%" fill="#ffffff"/>\n)
    heading = title || "#{@total} samples"
    heading += " · narrowed to #{ancestors.map { |a| display_name(a.func) }.join(' › ')}" unless ancestors.empty?
    out << %(<text x="10" y="18" font-size="13">#{escape(heading)}</text>\n)
    x = 10
    legend.each do |kind|
      out << %(<rect x="#{x}" y="26" width="12" height="12" rx="2" fill="#{self.class.color_of(kind)}"/>)
      out << %(<text x="#{x + 16}" y="36">#{escape(kind)}</text>\n)
      x += 16 + kind.size * 7 + 14
    end
    unless ancestors.empty?
      parent_path = focus.split(".")[0...-1].join(".")
      bar(out, focus_node, 0.0, width.to_f, 0, height, link&.call(parent_path))
    end
    render(out, focus_node, 0.0, base, width, height, focus_node.weight, focus || "", link)
    out << "</svg>\n"
  end

  private

  def render(out, node, x, level, width, height, scale, path, link)
    ordered_children(node).each_with_index do |child, i|
      w = child.weight * width.to_f / scale
      if w >= MIN_WIDTH
        child_path = path.empty? ? i.to_s : "#{path}.#{i}"
        bar(out, child, x, w, level, height, link&.call(child_path))
        render(out, child, x, level + 1, width, height, scale, child_path, link)
      end
      x += w
    end
  end

  def bar(out, node, x, w, level, height, href)
    y = height - 30 - (level + 1) * ROW
    name = display_name(node.func)
    title = "#{name} (#{label(node.func)}) #{pct(node.weight)}, #{node.weight} samples"
    out << %(<a href="#{escape(href)}">) if href
    out << %(<g><title>#{escape(title)}</title>)
    out << %(<rect x="#{x.round(2)}" y="#{y}" width="#{w.round(2)}" height="#{ROW - 1}" )
    out << %(fill="#{self.class.color_of(@kinds[node.func])}" rx="1"/>)
    if w > 30
      label = name[0, ((w - 6) / 6.5).floor]
      out << %(<text x="#{(x + 3).round(2)}" y="#{y + 12}">#{escape(label)}</text>)
    end
    out << "</g>"
    out << "</a>" if href
    out << "\n"
  end

  def escape(s) = s.to_s.gsub("&", "&amp;").gsub("<", "&lt;").gsub(">", "&gt;").gsub('"', "&quot;")
end
