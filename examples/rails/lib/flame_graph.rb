# frozen_string_literal: true

require "json"

# A flame graph and a summary from a Vernier profile.
#
# Each frame is colored by the kind of code it runs, so the picture itself answers the question
# "which part of a request runs in a converted extension, and does any C extension appear":
# - generated Ruby of a converted extension (`*_wasm.rb`);
# - a method of one of those gems written in C (`SQLite3::Statement#step` as a `<cfunc>`), which
#   appears only when the gem's own native extension is loaded;
# - dewasm cext, the host side of the Ruby C API (`cext/lib/dewasm_cext.rb`);
# - Ruby methods written in C (`<cfunc>`), which with the converted gems are all Ruby's own;
# - everything else: the application, Rails, and the other gems.
# The gem a converted or native frame belongs to is kept apart from the kind, for the summary.
class FlameGraph
  KINDS = {
    "Ruby (app, Rails, gems)" => "#f3b562",
    "converted gem" => "#3d7be8",
    "native gem" => "#1b2f6e",
    "dewasm cext" => "#20b2aa",
    "cfunc (Ruby core)" => "#9aa3ad"
  }.freeze
  # A C method of one of the four gems, by the namespace it is defined under.
  NATIVE_NAMESPACES = {
    "SQLite3" => "sqlite3", "Nokogiri" => "nokogiri", "Herb" => "herb", "BCrypt" => "bcrypt"
  }.freeze
  ROW = 16
  MIN_WIDTH = 0.4

  # A node of the call tree: `[weight, children]`, children keyed by function index.
  Node = Struct.new(:func, :weight, :children)

  attr_reader :total, :root, :func_names, :func_files, :self_weight

  # `profile` is the parsed JSON of a Vernier profile (the Firefox Profiler format).
  def self.from_vernier(profile)
    profile = JSON.parse(profile) if profile.is_a?(String)
    # The request runs in a server thread, so the thread with the most samples is the one wanted.
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

  # `stacks` is `[[function indices, root first], weight]` per sample.
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

  # `[kind, gem]`: the kind of code a function is, and for an extension's frame, its gem.
  def self.classify(file, name = "")
    case file
    when nil, /\A<cfunc>/
      gem = NATIVE_NAMESPACES[name[/\A([A-Z]\w*)(?:::|[#.])/, 1]]
      gem ? ["native gem", gem] : ["cfunc (Ruby core)", nil]
    when %r{/([a-z0-9_]+)_wasm\.rb\z}
      ["converted gem", Regexp.last_match(1).sub(/_(ext|native)\z/, "")]
    when %r{/dewasm_cext\.rb\z} then ["dewasm cext", nil]
    else ["Ruby (app, Rails, gems)", nil]
    end
  end

  def self.color_of(kind) = KINDS.fetch(kind)

  # Whether any frame is a native extension's C method.
  def native? = @kinds_seen.key?("native gem")

  # The kinds in the picture, in the legend's order.
  def legend = KINDS.keys.select { |kind| @kinds_seen[kind] }

  # `Sqlite3Ext#_f289_sqlite3VdbeExec` reads better as `Sqlite3Ext#sqlite3VdbeExec`.
  def display_name(func) = @func_names[func].sub(/#_f\d+_(?=\w)/, "#")

  def kind(func) = @kinds[func]

  def gem_of(func) = @gems[func]

  # The kind with the gem: `converted gem: sqlite3`.
  def label(func) = @gems[func] ? "#{@kinds[func]}: #{@gems[func]}" : @kinds[func]

  # `converted sqlite3` or `native sqlite3` for an extension's frame; `nil` for any other.
  def extension_label(func)
    @gems[func] && "#{@kinds[func].delete_suffix(' gem')} #{@gems[func]}"
  end

  def pct(w) = @total.zero? ? "0.0%" : format("%.1f%%", 100.0 * w / @total)

  # `[kind, weight]`, largest first.
  def by_kind = @kind_weight.sort_by { |_, w| -w }

  # `[extension label, weight]` of the converted or native extensions on the stack at all.
  def extensions_on_stack = @extension_on_stack.sort_by { |_, w| -w }

  # `[function, weight]`, largest self time first.
  def top_self(n = 15) = @self_weight.sort_by { |_, w| -w }.first(n)

  def cfuncs_seen
    @self_weight.keys.select { |f| @kinds[f] == "cfunc (Ruby core)" }.sort_by { |f| -@self_weight[f] }
  end

  def depth(node = @root, d = 0)
    node.children.values.map { |child| depth(child, d + 1) }.max || d
  end

  # Children in a stable order; a focus path names them by position in this order.
  def ordered_children(node) = node.children.values.sort_by { |c| @func_names[c.func] }

  # The node a focus path such as "0.3.1" names, with the frames above it.
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

  # The flame graph as SVG.
  # `focus` narrows the picture to one node; `link` turns a frame into a link that does so.
  # `link` is called with the focus path of a frame and returns a URL, or `nil` for no links.
  # The node the picture is narrowed to is the bottom row, the full width.
  # A click on it widens the picture by one level.
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

  # One frame: a box of width `w` at `x`, `level` rows above the bottom, linking to `href`.
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
