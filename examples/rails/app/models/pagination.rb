class Pagination
  attr_reader :number, :per, :total

  def initialize(param, per:, total:)
    @per = per
    @total = total
    @number = [ [ param.to_i, 1 ].max, pages ].min
  end

  def pages = [ (total.to_f / per).ceil, 1 ].max
  def offset = (number - 1) * per
  def first? = number == 1
  def last? = number == pages
  def slice(relation) = relation.offset(offset).limit(per)

  def from = total.zero? ? 0 : offset + 1
  def to = [ offset + per, total ].min
end
