-- The page inspector's own page: it shows the page of another pane, element by element. It is
-- bateri's, not a program's: it asks what it shows of the responder that stands where a program
-- would (`native_view::inspector`), with the kit's own components and its query module, so that
-- the kit is tried by the tool that is meant to look at it.
--
-- What the responder says: the topic `target` pushes `{ pane = , page = , gone = , stamp = }` (the
-- name of the pane looked at, whether it shows a page, whether it is gone, and how many pages its
-- script has made), the topic `costs` what its last frames cost, and the calls `children`, `node`
-- and `select` read and choose elements by their place in the tree (`{ path = { 1, 2 } }`); `close`
-- asks for this pane to be closed.
--
-- **What is asked of the target is asked under its stamp**: the context asks again what it asked
-- under another one, so the page reads the target's new page and never the old one's. A tree
-- is read a level at a time (a large page's whole tree is more than a message may carry), the
-- level of a folder when it is opened. An element is the tree's key by its name where it has
-- one and by its place where it has none, so that what is open and what is chosen follow the
-- element when the page is made again and its places change.

local ui = require("@pencere/ui")
local query = require("@pencere/query")

local u = ui.new()
local q = query.new()

-- The key of the page's own box, the tree's one root.
local ROOT = "@"

-- What the tree and the details are, kept as the components keep theirs: by what the page wrote.
local tree = { expanded = { [ROOT] = true }, cursor = nil, marks = {} }
local panes = { size = 280 }
local details = { cursor = nil, widths = {}, sort = nil }

-- The element chosen, as its key.
local chosen = nil

-- What is known of each element by its key: where it stands, what it is, its name, how many
-- children it has.
local nodes = { [ROOT] = { path = {}, shape = "box", count = 1 } }

-- The stamp of the target's page that what is asked is asked under.
local stamp = nil

pencere.subscribe("target", nil, function(value)
	q:set("target", nil, value)
	return true
end)

pencere.subscribe("costs", nil, function(value)
	q:set("costs", nil, value)
	return true
end)

-- The keyboard is the tree's from the first frame: it is walked with the arrows.
pencere.time.after(0, function()
	return u:focus(u:part("tree", "list"))
end)

-- What the program said last, from what the context knows of the key (nil before the first push).
local function said_of(read)
	return read and read.data
end

-- What the page says of what it looks at, where there is nothing to show of it.
local function words_of(target)
	if target == nil then
		return "Connecting to the pane…"
	elseif target.gone then
		return "The pane is gone."
	elseif not target.page then
		return "No page in " .. target.pane .. "."
	end
	return nil
end

-- A length in points, without the digits of a float.
local function points(value)
	return (string.format("%.2f", value):gsub("%.?0+$", ""))
end

local function box_words(rect)
	return points(rect.x) .. ", " .. points(rect.y) .. "   " .. points(rect.w) .. " × " .. points(rect.h)
end

-- Top, right, bottom, left.
local function sides_words(sides)
	local out = {}
	for index = 1, 4 do
		out[index] = points(sides[index])
	end
	return table.concat(out, "  ")
end

-- The key of an element: its name where it has one, its place where it has none (or where another
-- element of the same list has the same name).
local function key_of(child, taken)
	local place = "@" .. table.concat(child.path, ".")
	if child.id == nil then
		return place
	end
	local key = "#" .. child.id
	if taken[key] then
		return place
	end
	return key
end

-- The keys of a list of elements the responder gave, which are put in what is known of each.
local function keys_for(list)
	local keys, taken = {}, {}
	for _, child in ipairs(list) do
		local key = key_of(child, taken)
		taken[key] = true
		nodes[key] = child
		keys[#keys + 1] = key
	end
	return keys
end

-- The tree's way to the keys under one: nothing for what has none, an empty list for a folder that
-- is not open (its children are read when it is), the keys of what the responder says for one that is.
local function children_of(key)
	local node = nodes[key]
	if node == nil or (node.count or 0) == 0 then
		return nil
	end
	if tree.expanded[key] ~= true then
		return {}
	end
	local read = q:get("children", { path = node.path }, { stamp = stamp })
	local list = read.data and read.data.children
	if list == nil then
		return {}
	end
	return keys_for(list)
end

-- What the tree shows of an element: its name (or what it is, where it has none) and what it is.
local function item_of(key)
	local node = nodes[key]
	if key == ROOT then
		return { title = "page", hint = "root" }
	end
	-- A name is a number that the page's own word was made into: the first of it is what is shown.
	local tag = node.id and ("#" .. string.sub(node.id, 1, 8)) or nil
	return {
		title = node.name or node.shape,
		hint = tag or (node.name and node.shape or "no name"),
		tone = node.hidden and "dim" or nil,
	}
end

-- What the details say of an element, a line each: what it is, where its box is, its room and border,
-- how it was placed, and what a screen reader says of it.
local function rows_of(node)
	local rows = {}
	local function add(property, value)
		rows[#rows + 1] = { property, value }
	end
	add("element", node.shape .. (node.name and (" · " .. node.name) or ""))
	add("name", node.id and ("#" .. node.id) or "none")
	add("place", "[" .. table.concat(node.path, ", ") .. "]")
	add("box", box_words(node.rect))
	add("shows", box_words(node.clip))
	add("padding", sides_words(node.padding))
	add("border", node.border and (points(node.border.width) .. " pt") or "none")
	if node.content then
		add("content", points(node.content.w) .. " × " .. points(node.content.h))
		add("scrolled", points(node.scroll.x) .. ", " .. points(node.scroll.y))
	end
	local flags = {}
	if node.hidden then
		flags[#flags + 1] = "hidden"
	end
	if node.overlay then
		flags[#flags + 1] = "overlay"
	end
	if node.exiting then
		flags[#flags + 1] = "going out"
	end
	if #flags > 0 then
		add("flags", table.concat(flags, ", "))
	end
	local placing = node.placing
	if placing then
		add("layout", placing.display .. " " .. string.lower(placing.direction) .. (placing.wrap and " wrap" or ""))
		add("size", placing.width .. " × " .. placing.height)
		add("least", placing.min_width .. " × " .. placing.min_height)
		add("most", placing.max_width .. " × " .. placing.max_height)
		add("margin", sides_words(placing.margin))
		add("gap", points(placing.gap[1]) .. ", " .. points(placing.gap[2]))
		add("grow", points(placing.grow) .. (placing.shrink and ("  shrink " .. points(placing.shrink)) or ""))
		if placing.items then
			add("items", placing.items)
		end
		if placing.justify then
			add("justify", placing.justify)
		end
		add("overflow", placing.overflow)
	end
	for _, said in ipairs(node.said) do
		local words = said.role
		if said.words ~= "" then
			words = words .. ' "' .. said.words .. '"'
		end
		if said.state ~= "" then
			words = words .. " (" .. said.state .. ")"
		end
		add("reads as", words)
	end
	return rows
end

local COLUMNS = {
	{ key = "property", title = "Property", type = "text", width = 90 },
	{ key = "value", title = "Value", type = "text", width = 230 },
}

-- The line the last frame's cost makes.
local function cost_line(costs)
	local frames = costs and costs.frames
	local last = frames and frames[#frames]
	if last == nil then
		return "No frame drawn yet."
	end
	return string.format(
		"%d elements · script %.2f ms · layout %.2f ms · paint %.2f ms",
		costs.elements,
		last.script_ms,
		last.layout_ms,
		last.paint_ms
	)
end

-- A line of words in the page's quiet voice.
local function quiet(words)
	return pencere.div():size_full():p(12):child(pencere.text(words):color("dim"):text_size(12.5))
end

-- The tree: the page's own box and what is in it, a level at a time.
local function tree_view(target)
	local root = nodes[ROOT]
	if tree.expanded[ROOT] == true then
		-- Asked here, so that the first level is asked for before anything is opened.
		local read = q:get("children", { path = {} }, { stamp = stamp })
		if read.data == nil then
			return quiet(read.status == "failed" and "The page has not been drawn yet." or "Reading the page…")
		end
		root.count = #read.data.children
	end
	return pencere.div():size_full():child(u:tree("tree", {
		roots = { ROOT },
		children = children_of,
		item = function(key)
			if key == ROOT then
				return { title = target.pane, hint = "page" }
			end
			return item_of(key)
		end,
		state = tree,
	}))
end

-- What is known of the element chosen, in a grid.
local function details_view()
	local node = chosen and nodes[chosen]
	if node == nil then
		return quiet("Choose an element of the page.")
	end
	local read = q:get("node", { path = node.path }, { stamp = stamp })
	if read.data == nil then
		return quiet(read.status == "failed" and "That element is not in the page now." or "Reading the element…")
	end
	local rows = rows_of(read.data)
	return pencere.div():size_full():child(u:grid("details", {
		columns = COLUMNS,
		count = #rows,
		numbers = false,
		cell = function(row, column)
			local line = rows[row]
			return line and line[column.key == "property" and 1 or 2] or ""
		end,
		state = details,
	}))
end

-- The element the person chose: it is shown over the pane it is of.
local function choose(key)
	chosen = key
	local node = nodes[key]
	if node then
		pencere.call("select", { path = node.path }, nil)
	end
	return true
end

return {
	render = function(size, now, reduce_motion)
		u:begin(size, now, reduce_motion)
		local target = said_of(q:peek("target"))
		stamp = target and target.stamp
		local words = words_of(target)
		local header = pencere.div():row():items_center():gap(8):w_full()
			:child(pencere.text("Page inspector"):text_size(15):weight(600))
			:child(pencere.spacer())
			:child(u:icon_button("close", { icon = "close", label = "Close the inspector" }))
		local body
		if words then
			body = quiet(words)
		else
			body = u:split("panes", {
				state = panes,
				min = 160,
				max = 640,
				label = "Tree width",
				first = tree_view(target),
				second = details_view(),
			})
		end
		local root = pencere.div():size_full():bg("background"):color("foreground"):p(12):gap(8)
			:child(header)
			:child(pencere.div():grow(1):min_h(0):w_full():border(1, "border"):rounded(6):clip():child(body))
		if not words then
			root:child(pencere.text(cost_line(said_of(q:peek("costs")))):color("dim"):text_size(12))
		end
		return u:finish(root)
	end,

	settle = function()
		return u:settle()
	end,

	handle = function(event)
		if event.type == "key" and event.key == "escape" then
			pencere.call("close", nil, nil)
			return true
		end
		local reply, actions = u:handle(event)
		for _, action in actions do
			if action.from == "close" and action.kind == "press" then
				pencere.call("close", nil, nil)
				reply = pencere.merge(reply, true)
			elseif action.from == "tree" and (action.kind == "select" or action.kind == "expand") then
				-- A press on a row of a folder opens or closes it, and the element is the one
				-- chosen as well: every element but the leaves of the page is a folder.
				reply = pencere.merge(reply, choose(action.key))
			end
		end
		return reply
	end,
}
