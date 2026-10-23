-- The page API, part 2: elements and the chain that writes them,
-- `bateri.div():row():gap(8):child(bateri.text("hello"))`.
--
-- An element is a table: { kind = "box" | "text" | "spark", style = {...}, meta = {...}, kids = {...} }.
-- The host reads it, so the field names are the interface's (`bateri:view` `style`, `node`).

local Element = {}
Element.__index = Element
bateri.Element = Element

local function make(kind)
	return setmetatable({ kind = kind, style = {}, meta = {}, kids = {} }, Element)
end

-- A box: a column of its children until told otherwise.
function bateri.div()
	return make("box")
end

-- One line of text, in the font, size and colour of the boxes it is in.
function bateri.text(content)
	local element = make("text")
	element.text = tostring(content)
	return element
end

-- A sparkline of `values` (newest last), `color` the line.
function bateri.spark(values, color)
	local element = make("spark")
	element.values = values
	element.line = color
	return element
end

-- A box that takes the room left over: what pushes the elements after it to the end.
function bateri.spacer()
	return make("box"):grow(1)
end

-- --- Children ---------------------------------------------------------------------------------

-- Adds `child` after the ones already here (nothing, for nothing).
function Element:child(child)
	if child ~= nil then
		self.kids[#self.kids + 1] = child
	end
	return self
end

-- Adds every element of the list.
function Element:children(list)
	for _, child in ipairs(list) do
		self.kids[#self.kids + 1] = child
	end
	return self
end

-- --- Names and answers ------------------------------------------------------------------------

-- Names the element (a string, or a whole number), so that what is kept about it between
-- frames and what the host is asked of it can find it.
function Element:id(id)
	self.meta.id = id
	return self
end

-- Names the element and makes it something the pointer answers.
function Element:press(id)
	self.meta.id = id
	self.meta.pressable = true
	return self
end

-- The keyboard can be on this element (it is named too).
function Element:focusable()
	self.meta.focusable = true
	return self
end

-- The box is the room of a text field the host puts over the page (named too).
function Element:text_field()
	self.meta.text_field = true
	return self
end

-- --- Layout: the box --------------------------------------------------------------------------

function Element:row()
	local style = self.style
	style.display, style.direction = "flex", "row"
	return self
end

function Element:col()
	local style = self.style
	style.display, style.direction = "flex", "column"
	return self
end

function Element:wrap()
	self.style.wrap = true
	return self
end

-- A grid of `columns`: numbers are points, `bateri.fr(n)`, `bateri.range(least, n)`, "auto".
function Element:grid(columns)
	local style = self.style
	style.display = "grid"
	style.columns = columns
	return self
end

function Element:grid_rows(rows)
	self.style.rows = rows
	return self
end

function Element:span(columns)
	self.style.column_span = columns
	return self
end

function Element:hidden()
	self.style.display = "none"
	return self
end

function Element:items_start() self.style.items = "start" return self end
function Element:items_center() self.style.items = "center" return self end
function Element:items_end() self.style.items = "end" return self end
function Element:items_stretch() self.style.items = "stretch" return self end
function Element:justify_start() self.style.justify = "start" return self end
function Element:justify_center() self.style.justify = "center" return self end
function Element:justify_end() self.style.justify = "end" return self end
function Element:justify_between() self.style.justify = "between" return self end

function Element:gap(points)
	local style = self.style
	style.gap_x, style.gap_y = points, points
	return self
end

function Element:gap_x(points) self.style.gap_x = points return self end
function Element:gap_y(points) self.style.gap_y = points return self end

-- --- Layout: the element in its parent --------------------------------------------------------

function Element:grow(share) self.style.grow = share return self end
function Element:no_shrink() self.style.shrink = 0 return self end
function Element:w_full() self.style.width = { percent = 100 } return self end
function Element:h_full() self.style.height = { percent = 100 } return self end

function Element:size_full()
	return self:w_full():h_full()
end

function Element:w(points) self.style.width = points return self end
function Element:w_pct(percent) self.style.width = { percent = percent } return self end
function Element:h(points) self.style.height = points return self end

function Element:size(width, height)
	return self:w(width):h(height)
end

function Element:min_w(points) self.style.min_width = points return self end
function Element:min_h(points) self.style.min_height = points return self end
function Element:max_w(points) self.style.max_width = points return self end

-- Padding and margin are four numbers in the order top, right, bottom, left.
local function edges(style, name)
	local value = style[name]
	if not value then
		value = { 0, 0, 0, 0 }
		style[name] = value
	end
	return value
end

function Element:p(points)
	self.style.padding = { points, points, points, points }
	return self
end

function Element:px(points)
	local padding = edges(self.style, "padding")
	padding[2], padding[4] = points, points
	return self
end

function Element:py(points)
	local padding = edges(self.style, "padding")
	padding[1], padding[3] = points, points
	return self
end

function Element:pt(points) edges(self.style, "padding")[1] = points return self end
function Element:pr(points) edges(self.style, "padding")[2] = points return self end
function Element:pb(points) edges(self.style, "padding")[3] = points return self end
function Element:pl(points) edges(self.style, "padding")[4] = points return self end
function Element:mt(points) edges(self.style, "margin")[1] = points return self end
function Element:mr(points) edges(self.style, "margin")[2] = points return self end
function Element:mb(points) edges(self.style, "margin")[3] = points return self end
function Element:ml(points) edges(self.style, "margin")[4] = points return self end

-- Takes the box out of the flow; `inset` places it against the parent's box.
function Element:absolute()
	self.style.absolute = true
	return self
end

-- The distances of an absolute box's edges from its parent's (nil leaves an edge free).
function Element:inset(top, right, bottom, left)
	self.style.inset = { top = top, right = right, bottom = bottom, left = left }
	return self
end

-- Nothing is drawn outside the box.
function Element:clip()
	self.style.overflow = "hidden"
	return self
end

-- A box whose content is bigger than itself and moves behind it by `offset` points.
function Element:scroll_y(offset)
	local style = self.style
	style.overflow = "scroll"
	style.scroll_y = offset
	return self
end

-- --- Looks ------------------------------------------------------------------------------------

function Element:bg(color) self.style.fill = color return self end

function Element:border(width, color)
	self.style.border = { width = width, color = color }
	return self
end

function Element:rounded(points)
	self.style.radii = { points, points, points, points }
	return self
end

-- A radius per corner: top-left, top-right, bottom-right, bottom-left.
function Element:rounded_corners(radii)
	self.style.radii = radii
	return self
end

function Element:shadow(dx, dy, blur, color)
	self.style.shadow = { dx = dx, dy = dy, blur = blur, color = color }
	return self
end

-- --- Text (set here, inherited by the elements inside) ----------------------------------------

local function text_style(element)
	local style = element.style
	local text = style.text
	if not text then
		text = {}
		style.text = text
	end
	return text
end

function Element:text_size(points) text_style(self).size = points return self end
function Element:weight(weight) text_style(self).weight = weight return self end
function Element:color(color) text_style(self).color = color return self end
-- "system" or "terminal".
function Element:family(family) text_style(self).family = family return self end
-- "start", "center" or "end".
function Element:align(align) text_style(self).align = align return self end

function Element:truncate()
	self.style.truncate = true
	return self
end

-- The largest value a sparkline's top stands for.
function Element:peak(value)
	if self.kind == "spark" then
		self.ceiling = value
	end
	return self
end
