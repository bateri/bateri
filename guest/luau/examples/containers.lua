-- The container list of the engine's demo, as a script.
--
-- The page is the engine's own `Containers` (crates/bt-view/src/demo/containers*), written again
-- in Luau against the page API: the same elements in the same order with the same ids, numbers,
-- strings and tones, so that the tree this hands out is the tree the Rust page makes, and what
-- the person does to it (the pointer, the wheel, the keys, the filter, the menu) changes it the
-- same way. Where Rust's arithmetic is single-precision, the places where the two could part
-- ways (the made-up lines, the readings a program pushes) are rounded to it here too.
--
-- When a program opened the page, the page asks it for the numbers of the running containers
-- (it subscribes to `stats`) and takes what the program pushes: `{"readings": [{"name", "cpu",
-- "memory", "share"}, ..]}`, a reading for each container once a second.
--
-- One thing more than the list: Shift-U sends the program a large payload (4 MiB, in chunks of
-- 48 KiB), as an example of a page with a lot to say to a program that reads when it can. The
-- program may not have read what it was told before: `bateri.send` answers that, the page keeps
-- the chunk and goes on when it is called in `room`. It says at most a few chunks in a call,
-- going on in the next frame.

local div, text, spark, spacer = bateri.div, bateri.text, bateri.spark, bateri.spacer
local alpha = bateri.alpha
local Rows, Ui = bateri.Rows, bateri.Ui

-- --- Numbers ---------------------------------------------------------------------------------

local TWO32 = 4294967296

-- `x` as the single-precision number Rust would have it as.
local f32 = bateri.f32

local function finite(x)
	return x == x and x ~= math.huge and x ~= -math.huge
end

-- `a * b` modulo 2^32 for two numbers below 2^32: the product itself would not fit a double.
local function mul32(a, b)
	local high, low = a // 65536, a % 65536
	return ((high * b) % TWO32 * 65536 + low * b) % TWO32
end

-- Thirty samples of a made-up CPU line: around `base`, with a spike of up to `spike` now and
-- then. A fixed generator: the page looks the same every time it is opened. Every step is
-- rounded to single precision, as the Rust generator's are.
local SPIKE_AT = f32(0.82)

local function wave(seed, base, spike)
	base, spike = f32(base), f32(spike)
	local low, high, factor, share = f32(0.8), f32(0.2), f32(0.35), SPIKE_AT
	local state = (mul32(seed, 2654435761) + 1) % TWO32
	local line = {}
	for index = 1, 30 do
		state = (mul32(state, 1664525) + 1013904223) % TWO32
		local r = (state // 256) / 16777216
		-- `state` turned left by 13 bits.
		local turned = (state * 8192) % TWO32 + state // 524288
		local s = (turned // 256) / 16777216
		local bump
		if s > share then
			bump = f32(spike * r)
		else
			bump = f32(f32(base * factor) * r)
		end
		line[index] = f32(f32(base * f32(low + f32(high * r))) + bump)
	end
	return line
end

local function contains(list, value)
	for _, item in ipairs(list) do
		if item == value then
			return true
		end
	end
	return false
end

-- The characters Unicode counts as white space, which is what the Rust page trims the filter of.
local function is_space(code)
	return (code >= 0x09 and code <= 0x0D)
		or code == 0x20
		or code == 0x85
		or code == 0xA0
		or code == 0x1680
		or (code >= 0x2000 and code <= 0x200A)
		or code == 0x2028
		or code == 0x2029
		or code == 0x202F
		or code == 0x205F
		or code == 0x3000
end

local function trim(words)
	local ok, trimmed = pcall(function()
		local first, last = 1, #words
		while first <= last and is_space(utf8.codepoint(words, first)) do
			first = utf8.offset(words, 2, first)
		end
		while last >= first do
			local start = utf8.offset(words, -1, last + 1)
			if not is_space(utf8.codepoint(words, start)) then
				break
			end
			last = start - 1
		end
		return string.sub(words, first, last)
	end)
	if ok then
		return trimmed
	end
	-- Text that is not UTF-8 is trimmed of the plain white space only.
	return (string.match(words, "^%s*(.-)%s*$"))
end

-- --- Sizes and names ---------------------------------------------------------------------------

-- The rail folds away under this page width, points.
local RAIL_BELOW = 780
local RAIL_WIDTH = 188
-- The columns the list drops as it narrows, by the list's width, points.
local MEDIUM_BELOW = 860
local NARROW_BELOW = 660
local TINY_BELOW = 470
-- One row's height, points.
local ROW = 32
-- The room above a project's row, points.
local GROUP_GAP = 6
-- The room before the list's first row and after its last, points.
local LIST_LEAD = 6
local LIST_TRAIL = 8

local LIST = "containers list"
local SEARCH = "containers filter"
local FIELD = "containers filter field"
local GROUP_BUTTON = "containers group"
-- The button that would sort the list: there is one order in the fixed list, so it only looks it.
local SORT_BUTTON = "containers sort"

-- The roles of an element's fades.
local HOVERED, CHOSEN = 0, 1

-- The fade of an element in a role: kept by a name of its own, which no element has.
local function role(id, which)
	return id .. "\0" .. which
end

local STATUSES = { "all", "running", "stopped", "issues" }
local STATUS_LABELS = { all = "All", running = "Running", stopped = "Stopped", issues = "Issues" }

local function status_id(status)
	return "containers status " .. status
end

-- What names a row of the list: a project's, or a container's.
local function group_key(project)
	if project then
		return { group = true, project = project, id = "containers group " .. project }
	end
	return { group = true, id = "containers group standalone" }
end

local function container_key(name)
	return { name = name, id = "containers container " .. name }
end

-- --- Containers --------------------------------------------------------------------------------

local function is_up(c)
	return c.state.kind ~= "exited"
end

local function has_issue(c)
	return c.state.kind == "unhealthy" or (c.state.kind == "exited" and c.state.code ~= 0)
end

local function status_shows(status, c)
	if status == "running" then
		return is_up(c)
	elseif status == "stopped" then
		return not is_up(c)
	elseif status == "issues" then
		return has_issue(c)
	end
	return true
end

-- Runs again (or for the first time): up since now, with a quiet line to show.
local function begin(c, age, kind)
	if not is_up(c) or #c.history == 0 then
		local seed = 7
		for index = 1, #c.name do
			seed = (mul32(seed, 31) + string.byte(c.name, index)) % TWO32
		end
		c.history = wave(seed, 0.3, 1.0)
		c.memory = { "12MiB", f32(0.06) }
		c.cpu = f32(0.3)
	end
	c.state = { kind = kind }
	c.age = age
end

-- Ended with `code`, `age` ago.
local function finish(c, age, code)
	c.state = { kind = "exited", code = code }
	c.age = age
	c.cpu = 0
end

-- A made-up id, the same every time for the same name: twelve hex digits of the name's FNV-1a
-- hash. The hash is 64 bits and a double holds 53, so it is kept in two halves.
local function xor8(a, b)
	local out, bit = 0, 1
	for _ = 1, 8 do
		if a % 2 ~= b % 2 then
			out += bit
		end
		a //= 2
		b //= 2
		bit *= 2
	end
	return out
end

local function container_id(c)
	local high, low = 0xcbf29ce4, 0x84222325
	for index = 1, #c.name do
		low = low - low % 256 + xor8(low % 256, string.byte(c.name, index))
		-- Times 0x100000001b3: 0x1b3 on the whole, and the low 24 bits again 40 bits up.
		local product = low * 0x1b3
		local carry = product // TWO32
		high = (high * 0x1b3 + carry + (low % 16777216) * 256) % TWO32
		low = product % TWO32
	end
	return string.format("%04x%08x", high % 65536, low)
end

-- --- The columns -------------------------------------------------------------------------------

local function width_of(list)
	if list < TINY_BELOW then
		return "tiny"
	elseif list < NARROW_BELOW then
		return "narrow"
	elseif list < MEDIUM_BELOW then
		return "medium"
	end
	return "wide"
end

local function shows_image(width)
	return width == "wide" or width == "medium"
end

local function shows_ports(width)
	return width == "wide"
end

local function shows_memory(width)
	return width ~= "tiny"
end

-- The grid's columns: the dot, the name, then what fits.
local function tracks(width)
	local list = { 18 }
	if width == "wide" or width == "medium" then
		list[#list + 1] = bateri.range(120, 1.5)
	elseif width == "narrow" then
		list[#list + 1] = bateri.range(100, 1)
	else
		list[#list + 1] = bateri.range(90, 1)
	end
	if shows_image(width) then
		list[#list + 1] = bateri.range(width == "wide" and 90 or 80, 1)
	end
	list[#list + 1] = 96
	if shows_memory(width) then
		list[#list + 1] = 110
	end
	if shows_ports(width) then
		list[#list + 1] = bateri.range(64, 0.8)
	end
	list[#list + 1] = 128
	return list
end

-- --- Small parts -------------------------------------------------------------------------------

-- A key cap.
local function kbd(label, size)
	return div():row():items_center():justify_center():min_w(18):h(18):px(4):rounded(4)
		:bg("raised2"):border(1, "line"):no_shrink()
		:child(text(label):text_size(size):color("dim"))
end

-- A round dot of `size` points in `tone`; `ring` adds the soft halo of a live one.
local function dot(size, tone, ring)
	local core = div():size(size, size):rounded(size / 2):bg(tone)
	if ring then
		local halo = size + 6
		return div():row():items_center():justify_center():size(halo, halo):rounded(halo / 2)
			:bg(alpha(tone, 0.18)):child(core)
	end
	return core
end

-- The state's dot: solid with a halo for a container that is up, a hollow ring for one that has
-- ended (in the error colour when it ended badly).
local function state_dot(state, tone)
	if state.kind == "exited" then
		return div():size(10, 10):rounded(5):border(1.5, tone)
	end
	return dot(10, tone, true)
end

-- A value at the right of a meter: the number, in a box of its own width.
local function value(content)
	return div():min_w(42):no_shrink():child(text(content):text_size(11.5):color("dim"):align("end"))
end

-- A cell with nothing to say: a quiet dash at its end.
local function dash()
	return text("\u{2013}"):color("quiet"):align("end")
end

-- A small label in a tone, on the tone's own faint ground.
local function tag(label, tone)
	return div():row():items_center():h(18):px(6):rounded(4):bg(alpha(tone, 0.13)):no_shrink()
		:child(text(label):text_size(10.5):color(tone))
end

-- A published port: the tool's link colour on its faint ground.
local function port_chip(port)
	return div():row():items_center():h(20):px(6):rounded(4):bg(alpha("info", 0.11)):no_shrink()
		:child(text(":" .. port):text_size(11):color("info"))
end

-- The line of keys at the bottom: as many of them as the page's width has room for.
local HINTS = {
	{ "s", "stop" },
	{ "r", "restart" },
	{ "e", "open a shell" },
	{ "l", "logs" },
	{ ".", "more" },
	{ "space", "mark" },
}

local function hint(key, label)
	return div():row():items_center():gap(5):h(20):px(4):no_shrink()
		:child(kbd(key, 10):min_w(16):h(16))
		:child(text(label):text_size(11):color("quiet"))
end

local function status_line(width)
	local keys
	if width >= 760 then
		keys = 6
	elseif width >= 560 then
		keys = 4
	else
		keys = 2
	end
	local hints = {}
	for index = 1, keys do
		hints[index] = hint(HINTS[index][1], HINTS[index][2])
	end
	return div():col():no_shrink():bg("raised1")
		:child(div():h(1):bg("line"))
		:child(
			div():row():items_center():gap(10):h(25):px(8):clip()
				:child(div():row():items_center():gap(4):min_w(0):clip():children(hints))
				:child(spacer())
				:child(hint(":", "commands"))
				:child(hint("?", ""))
				:child(dot(8, "success", false))
		)
end

-- --- The page ----------------------------------------------------------------------------------

local Containers = {}
Containers.__index = Containers

-- The reading's memory text may be this long, in characters: what a row has room for, and a
-- bound on what a program can make the page hold.
local MEMORY_TEXT = 16
local HISTORY = 30

-- A small, made-up Docker host: a shop with a container in each state, a blog that is stopped, a
-- project folded away and one container of no project.
function Containers.fixture()
	local function running(name, project, image, cpu, history, memory, ports, age)
		return {
			name = name,
			project = project,
			image = image,
			state = { kind = "running" },
			cpu = f32(cpu),
			history = history,
			memory = { memory[1], f32(memory[2]) },
			ports = ports,
			age = age,
		}
	end
	local function stopped(name, project, image, code, age)
		return {
			name = name,
			project = project,
			image = image,
			state = { kind = "exited", code = code },
			cpu = 0,
			history = {},
			memory = { "", 0 },
			ports = {},
			age = age,
		}
	end
	local function with_state(c, kind)
		c.state = { kind = kind }
		return c
	end
	local shop, blog, stats = "shop", "blog", "stats"
	local containers = {
		running("api", shop, "shop/api:2.4", 0.9, wave(3, 0.8, 4.0), { "112MiB", 0.22 }, { 8080 }, "up 3m"),
		running("cache", shop, "redis:alpine", 0.3, wave(5, 0.4, 2.5), { "7.2MiB", 0.04 }, { 6379 }, "up 3m"),
		running("db", shop, "postgres:17-alpine", 1.2, wave(11, 1.0, 6.0), { "203MiB", 0.48 }, { 5432 }, "up 3m"),
		stopped("mailer", shop, "mailhog/mailhog", 1, "2m ago"),
		with_state(
			running("search", shop, "opensearch:2.14", 0.0, wave(2, 0.1, 9.0), { "976MiB", 0.81 }, { 9200 }, "up 3m"),
			"unhealthy"
		),
		running("web", shop, "nginx:alpine", 0.0, wave(7, 0.0, 3.0), { "6.1MiB", 0.03 }, { 8081 }, "up 3m"),
		with_state(
			running("worker", shop, "shop/worker:2.4", 37.6, wave(13, 30.0, 14.0), { "936KiB", 0.01 }, {}, "up 12s"),
			"starting"
		),
		stopped("ghost", blog, "ghost:5-alpine", 0, "3d ago"),
		stopped("mysql", blog, "mysql:8", 0, "3d ago"),
		running("collector", stats, "prom/node-exporter", 0.1, wave(17, 0.1, 0.5), { "18.4MiB", 0.09 }, { 9100 }, "up 2d"),
		running("grafana", stats, "grafana/grafana", 0.4, wave(19, 0.4, 1.2), { "91MiB", 0.19 }, { 3000 }, "up 2d"),
		stopped("alerts", stats, "prom/alertmanager", 0, "2d ago"),
		running("toolbox", nil, "busybox", 0.0, wave(23, 0.0, 0.2), { "128KiB", 0.0 }, {}, "up 2m"),
	}
	local page = setmetatable({
		containers = containers,
		-- The row the cursor is on.
		cursor = container_key("api"),
		-- The containers marked, to act on many at once.
		marked = {},
		-- Projects whose rows are folded away.
		collapsed = { "stats" },
		-- What the filter field says, and which state of container to show.
		query = "",
		status = "all",
		-- Whether the list is grouped by project.
		grouped = true,
		-- What the page remembers between builds: the hover, the scroll, the fades.
		ui = Ui.new(),
		-- Where the pointer last was over the page: the hover is asked again where it stands when
		-- the page moves under it.
		pointer = nil,
		-- How tall the list's window was when the page was last laid out, points.
		list_view = 400,
		-- What the entries of the menu that is up do, by entry; and the containers they do it to.
		menu = { acts = {}, targets = {} },
		-- The modifiers held when the last button went down.
		press_mods = {},
		-- Reduce Motion, as of the last frame: a change snaps.
		reduce_motion = false,
	}, Containers)
	-- The choice starts shown, not fading in.
	page.ui.fades:snap(role(page.cursor.id, CHOSEN), 1)
	return page
end

function Containers:up()
	local count = 0
	for _, c in ipairs(self.containers) do
		if is_up(c) then
			count += 1
		end
	end
	return count
end

function Containers:issues()
	local count = 0
	for _, c in ipairs(self.containers) do
		if has_issue(c) then
			count += 1
		end
	end
	return count
end

-- --- What the page shows -------------------------------------------------------------------------

-- Whether the filter lets `c` through: its state, and the words in the field among its name,
-- project, image and published ports.
function Containers:shows(c)
	if not status_shows(self.status, c) then
		return false
	end
	local query = string.lower(trim(self.query))
	if query == "" then
		return true
	end
	if string.find(string.lower(c.name), query, 1, true) then
		return true
	end
	if c.project and string.find(string.lower(c.project), query, 1, true) then
		return true
	end
	if string.find(string.lower(c.image), query, 1, true) then
		return true
	end
	for _, port in ipairs(c.ports) do
		if string.find(tostring(port), query, 1, true) then
			return true
		end
	end
	return false
end

-- The rows of the list: each project's, with the containers under it unless folded, or
-- ungrouped, the containers by name. A filter at work unfolds every project, so what it finds
-- is not hidden in a folded one.
function Containers:rows()
	local containers = self.containers
	local shown = {}
	for at, c in ipairs(containers) do
		if self:shows(c) then
			shown[#shown + 1] = at
		end
	end
	local function container_row(at)
		return { key = container_key(containers[at].name), what = { container = at } }
	end
	local rows = {}
	if not self.grouped then
		table.sort(shown, function(a, b)
			return containers[a].name < containers[b].name
		end)
		for _, at in ipairs(shown) do
			rows[#rows + 1] = container_row(at)
		end
		return rows
	end
	local filtering = self.status ~= "all" or trim(self.query) ~= ""
	local done = {}
	for _, at in ipairs(shown) do
		local project = containers[at].project
		local mark = project or "\0standalone"
		if not done[mark] then
			done[mark] = true
			local members = {}
			for _, member in ipairs(shown) do
				if containers[member].project == project then
					members[#members + 1] = member
				end
			end
			local folded = not filtering and project ~= nil and contains(self.collapsed, project)
			rows[#rows + 1] = {
				key = group_key(project),
				what = { group = true, project = project, members = members, folded = folded },
			}
			if not folded then
				for _, member in ipairs(members) do
					rows[#rows + 1] = container_row(member)
				end
			end
		end
	end
	return rows
end

-- How many containers each state of the filter would show.
function Containers:counts()
	local counts = { all = #self.containers, running = 0, stopped = 0, issues = 0 }
	for _, c in ipairs(self.containers) do
		for _, status in ipairs({ "running", "stopped", "issues" }) do
			if status_shows(status, c) then
				counts[status] += 1
			end
		end
	end
	return counts
end

-- How lit the element named `id` is from the pointer being over it, 0 to 1.
function Containers:glow(id)
	return self.ui.fades:level(role(id, HOVERED))
end

-- How tall a row stands in the list, with the room above a project's.
local function row_height(row)
	if row.what.group then
		return ROW + GROUP_GAP
	end
	return ROW
end

-- Where the rows stand in the list: their heights, and the room around them.
local function places(rows)
	local heights = {}
	for index, row in ipairs(rows) do
		heights[index] = row_height(row)
	end
	return Rows.new(heights):padded(LIST_LEAD, LIST_TRAIL)
end

-- --- The page itself ---------------------------------------------------------------------------

-- The page for a pane of `size` points.
function Containers:view(size)
	local rail = size.w >= RAIL_BELOW
	local list_width = size.w - (rail and RAIL_WIDTH + 1 or 0) - 16
	local width = width_of(list_width)
	local main = div():col():grow(1):min_w(0):clip()
		:child(self:header())
		:child(self:toolbar())
		:child(self:list(width, size.h))
	local body = div():row():grow(1):min_h(0):clip()
		:child(rail and self:rail() or nil)
		:child(main)
	return div():col():size_full():bg("background"):text_size(12.5):color("foreground")
		:child(body)
		:child(status_line(size.w))
end

-- --- The rail ----------------------------------------------------------------------------------

local function rail_item(label, current, count, badge)
	local row = div():row():items_center():gap(10):h(30):px(8):rounded(6):no_shrink()
		:color(current and "foreground" or "dim")
		:child(text(label):grow(1):truncate())
	if current then
		row = row:bg(alpha("accent", 0.13)):border(1, alpha("accent", 0.24))
	end
	if badge then
		return row:child(
			div():row():items_center():justify_center():min_w(16):h(16):px(4):rounded(8):bg("error")
				:child(text(tostring(badge)):text_size(10):weight(650):color("background"))
		)
	elseif count then
		return row:child(text(count):text_size(11):color("quiet"))
	end
	return row
end

local function meter(label, values, tone, reading)
	return div():row():items_center():gap(6):text_size(10.5)
		:child(text(label):color("quiet"):w(26))
		:child(spark(values, tone):size(70, 14))
		:child(text(reading):color("dim"):weight(500):grow(1):align("end"))
end

function Containers:rail()
	local issues = self:issues()
	local nav = div():col():gap(2)
		:child(rail_item("Containers", true, nil, issues > 0 and issues or nil))
		:child(rail_item("Images", false, "14", nil))
		:child(rail_item("Volumes", false, "6", nil))
		:child(rail_item("Networks", false, "4", nil))
		:child(rail_item("System", false, nil, nil))
		:child(rail_item("Activity", false, nil, nil))
	local engine = div():col():gap(6):p(8):rounded(8):bg(alpha("background", 0.55)):border(1, "line")
		:child(
			div():row():items_center():gap(7):text_size(11):color("dim")
				:child(dot(7, "success", false))
				:child(text("Alpine Linux v3.24"):truncate())
		)
		:child(meter("cpu", wave(29, 30.0, 12.0), "accent", "39%"))
		:child(meter("mem", wave(31, 40.0, 3.0), "success", "36.8MiB"))
		:child(text(self:up() .. " running \u{b7} 6 cpu"):text_size(10.5):color("quiet"):truncate())
	return div():row():no_shrink()
		:child(
			div():col():w(RAIL_WIDTH):min_h(0):clip():px(8):pt(10):pb(8):bg("raised1")
				:child(nav)
				:child(spacer())
				:child(engine)
		)
		:child(div():w(1):bg("line"))
end

-- --- The section's top -------------------------------------------------------------------------

function Containers:header()
	local function ghost(label)
		return div():row():items_center():h(28):px(10):rounded(6):color("dim"):child(text(label))
	end
	return div():row():items_center():gap(10):px(14):pt(10):pb(8):min_h(46)
		:child(text("Containers"):text_size(13.5):weight(650):no_shrink())
		:child(
			text(string.format("%d running \u{b7} %d total", self:up(), #self.containers))
				:text_size(11.5):color("quiet"):truncate()
		)
		:child(spacer())
		:child(ghost("Pull"))
		:child(ghost("Clean up"))
end

function Containers:toolbar()
	-- The host's text field stands in the box: the page draws the box and the key that reaches it,
	-- and keeps the room (`FIELD`).
	local search = div():row():items_center():gap(6):h(28):w(220):min_w(120):px(8):rounded(6)
		:bg("raised1"):border(1, "line"):press(SEARCH)
		:child(div():id(FIELD):text_field():grow(1):min_w(0):h(18))
		:child(kbd("/", 10.5))
	local counts = self:counts()
	local function option(status, count, tone)
		local active = self.status == status
		local id = status_id(status)
		local glow = self:glow(id)
		local cell = div():row():items_center():gap(6):h(24):px(9):rounded(6):press(id)
			:color((active or glow > 0) and "foreground" or "dim")
		if active then
			cell = cell:bg("raised2"):border(1, "line"):shadow(0, 1, 2, alpha("shadow", 0.5))
		elseif glow > 0 then
			cell = cell:bg(alpha("foreground", 0.07 * glow))
		end
		local count_color = tone or (active and "dim" or "quiet")
		return cell:child(text(STATUS_LABELS[status]))
			:child(text(tostring(count)):text_size(10.5):color(count_color))
	end
	local segmented = div():row():p(2):gap(1):rounded(8):bg("raised1"):border(1, "line"):no_shrink()
		:child(option("all", counts.all, nil))
		:child(option("running", counts.running, nil))
		:child(option("stopped", counts.stopped, nil))
		:child(option("issues", counts.issues, "error"))
	local function button(label, id, pressed)
		local glow = self:glow(id)
		local look = div():row():items_center():h(28):px(9):rounded(6):no_shrink()
			:color((pressed or glow > 0) and "foreground" or "dim")
		if pressed then
			look = look:bg(alpha("accent", 0.13)):border(1, alpha("accent", 0.4))
		elseif glow > 0 then
			look = look:bg(alpha("foreground", 0.07 * glow))
		end
		-- A button that does nothing does not answer the pointer either.
		if id == SORT_BUTTON then
			look = look:id(id)
		else
			look = look:press(id)
		end
		return look:child(text(label):text_size(11.5))
	end
	return div():row():wrap():items_center():gap(6):px(14):pb(8)
		:child(search)
		:child(segmented)
		:child(button("Group", GROUP_BUTTON, self.grouped))
		:child(button("Name \u{2193}", SORT_BUTTON, false))
end

-- --- The list ----------------------------------------------------------------------------------

function Containers:list(width, view)
	local function head(label, active)
		local words = string.upper(label)
		return text(active and (words .. " \u{25be}") or words):truncate()
	end
	local heads = { div():w(0), head("Name", true) }
	if shows_image(width) then
		heads[#heads + 1] = head("Image", false)
	end
	heads[#heads + 1] = head("CPU", false)
	if shows_memory(width) then
		heads[#heads + 1] = head("Memory", false)
	end
	if shows_ports(width) then
		heads[#heads + 1] = head("Ports", false)
	end
	heads[#heads + 1] = head("Status", false):align("end")
	local top = div():grid(tracks(width)):gap_x(10):items_center():h(28):px(16):no_shrink()
		:text_size(10.5):color("quiet"):children(heads)

	-- Only the rows in view are made; the rest of the list's length is two spacers.
	local rows = self:rows()
	local standing = places(rows)
	local offset = self.ui:scroll_offset(LIST)
	local first, last = standing:window(offset, view, 2 * ROW)
	local body = div():col():id(LIST):focusable():grow(1):min_h(0):px(8):scroll_y(offset)
		:child(div():h(standing:top(first)):no_shrink())
	if #rows == 0 then
		body = body:child(self:nothing())
	end
	for index = first, last do
		body = body:child(self:row(rows[index], width))
	end
	body = body:child(div():h(standing:total() - standing:top(last + 1)):no_shrink())

	return div():col():grow(1):min_h(0):clip()
		:child(top)
		:child(div():h(1):no_shrink():bg(alpha("line", 0.6)))
		:child(body)
end

-- What the list says where no container is shown.
function Containers:nothing()
	local reason
	if self.query == "" then
		reason = "No " .. string.lower(STATUS_LABELS[self.status]) .. " containers."
	else
		reason = "No container matches \u{201c}" .. self.query .. "\u{201d}."
	end
	return div():col():items_center():gap(4):pt(48):no_shrink()
		:child(text(reason):color("dim"))
		:child(text("Change the filter, or press Esc in the field to leave it."):text_size(11):color("quiet"))
end

-- The row dressed: a chosen one on the accent with its border, one the pointer is over on a
-- faint light.
local function dress(row, chosen, hovered)
	if chosen > 0 then
		return row:bg(alpha("accent", 0.13 * chosen)):border(1, alpha("accent", 0.55 * chosen))
	elseif hovered > 0 then
		return row:bg(alpha("foreground", 0.055 * hovered))
	end
	return row
end

local function group_row(id, project, members, folded, chosen, hovered)
	local up, issues = 0, 0
	local stack = div():row():gap(2)
	for index, m in ipairs(members) do
		if is_up(m) then
			up += 1
		end
		if has_issue(m) then
			issues += 1
		end
		if index <= 12 then
			local color
			if has_issue(m) then
				color = "error"
			elseif is_up(m) then
				color = "success"
			else
				color = alpha("quiet", 0.4)
			end
			stack = stack:child(div():size(5, 10):rounded(2):bg(color))
		end
	end
	local summary
	if issues > 0 then
		summary = string.format("%d/%d up \u{b7} %d with issues", up, #members, issues)
	else
		summary = string.format("%d/%d up", up, #members)
	end
	return dress(
		div():row():items_center():gap(8):h(ROW):mt(GROUP_GAP):px(8):rounded(6):no_shrink():press(id):color("dim")
			:child(
				div():row():justify_center():w(18)
					:child(text(folded and "\u{25b8}" or "\u{25be}"):text_size(11))
			)
			:child(text(project or "Standalone"):weight(650):color("foreground"):no_shrink())
			:child(stack)
			:child(text(summary):text_size(11.5):color("quiet"):truncate()),
		chosen,
		hovered
	)
end

-- A container's row, under its project's: the name is set in from the project's.
local function container_row(id, c, width, chosen, hovered, marked)
	local up = is_up(c)
	local kind, code = c.state.kind, c.state.code
	local tone
	if kind == "running" then
		tone = "success"
	elseif kind == "starting" or kind == "paused" then
		tone = "warning"
	elseif kind == "unhealthy" then
		tone = "error"
	elseif code ~= 0 then
		tone = "error"
	else
		tone = "quiet"
	end
	local label, label_tone
	if kind == "unhealthy" then
		label, label_tone = "unhealthy", "error"
	elseif kind == "starting" then
		label, label_tone = "starting", "warning"
	elseif kind == "paused" then
		label, label_tone = "paused", "warning"
	elseif kind == "exited" and code ~= 0 then
		label, label_tone = string.format("exited %d", code), "error"
	end
	local name = div():row():items_center():gap(8):min_w(0):pl(14)
		:child(
			up and text(c.name):weight(600):truncate()
				or text(c.name):weight(500):color("dim"):truncate()
		)
	-- Where the name has little room, the state is the dot's colour alone.
	if width ~= "tiny" and width ~= "narrow" and label then
		name = name:child(tag(label, label_tone))
	end

	local cells = {
		div():row():items_center():justify_center():child(state_dot(c.state, tone)),
		name,
	}
	if shows_image(width) then
		cells[#cells + 1] = text(c.image):color("dim"):truncate()
	end
	if up then
		cells[#cells + 1] = div():row():items_center():justify_between():gap(6)
			:child(spark(c.history, "accent"):size(44, 14):no_shrink())
			:child(value(string.format("%.1f%%", c.cpu)))
	else
		cells[#cells + 1] = dash()
	end
	if shows_memory(width) then
		if up then
			local share = math.clamp(c.memory[2], 0, 1)
			local color
			if share > 0.85 then
				color = "error"
			elseif share > 0.65 then
				color = "warning"
			else
				color = "quiet"
			end
			cells[#cells + 1] = div():row():items_center():gap(6)
				:child(
					div():h(4):grow(1):min_w(0):rounded(2):bg("raised2"):clip()
						:child(div():h_full():w_pct(math.max(f32(share * 100), 2)):rounded(2):bg(color))
				)
				:child(value(c.memory[1]))
		else
			cells[#cells + 1] = dash()
		end
	end
	if shows_ports(width) then
		local ports = div():row():items_center():gap(4):min_w(0):clip()
		for index = 1, math.min(#c.ports, 2) do
			ports = ports:child(port_chip(c.ports[index]))
		end
		if #c.ports > 2 then
			ports = ports:child(text("+" .. (#c.ports - 2)):text_size(11):color("quiet"))
		end
		cells[#cells + 1] = ports
	end
	cells[#cells + 1] = text(c.age):text_size(11.5):color("dim"):align("end"):truncate()

	local row = div():grid(tracks(width)):gap_x(10):items_center():h(ROW):px(8):rounded(6):no_shrink()
		:press(id):children(cells)
	if marked then
		-- A bar at the row's edge: the container is marked, to be acted on with the others.
		row = row:child(div():absolute():inset(8, nil, 8, 1):w(3):rounded(1.5):bg("accent"))
	end
	return dress(row, chosen, hovered)
end

-- The element of one row of the list.
function Containers:row(row, width)
	local id = row.key.id
	local chosen = self.ui.fades:level(role(id, CHOSEN))
	local hovered = self.ui.fades:level(role(id, HOVERED))
	if row.what.group then
		local members = {}
		for index, at in ipairs(row.what.members) do
			members[index] = self.containers[at]
		end
		return group_row(id, row.what.project, members, row.what.folded, chosen, hovered)
	end
	local c = self.containers[row.what.container]
	return container_row(id, c, width, chosen, hovered, contains(self.marked, c.name))
end

-- --- Moving the fades --------------------------------------------------------------------------

function Containers:fade_to(id, target)
	if self.reduce_motion then
		self.ui.fades:snap(id, target)
	else
		self.ui.fades:to(id, target)
	end
end

-- The cursor goes to `key` (nowhere for nil): the row it left fades out and the one it came to
-- fades in. Whether it moved.
function Containers:set_cursor(key)
	local was = self.cursor
	if (was == nil and key == nil) or (was ~= nil and key ~= nil and was.id == key.id) then
		return false
	end
	if was then
		self:fade_to(role(was.id, CHOSEN), 0)
	end
	if key then
		self:fade_to(role(key.id, CHOSEN), 1)
	end
	self.cursor = key
	return true
end

function Containers:hover_to(laid, at)
	local was = self.ui.hover
	if not self.ui:point(laid, at) then
		return false
	end
	if was then
		self:fade_to(role(was, HOVERED), 0)
	end
	if self.ui.hover then
		self:fade_to(role(self.ui.hover, HOVERED), 1)
	end
	return true
end

-- The step of the fades to `now`; whether any is still on the way. Under Reduce Motion every
-- change is already at its end.
function Containers:advance(now, reduce_motion)
	self.reduce_motion = reduce_motion
	return self.ui.fades:advance(now, reduce_motion)
end

-- Whether anything of the page is still moving: the host draws another frame while it is.
function Containers:animating()
	return self.ui.fades:moving()
end

-- The page was laid out as `laid`: what depends on where things stand is brought up to date, the
-- scrolled list kept within its reach, the window's height, what the pointer is over now that
-- the page moved under it. Whether the page has to be built and laid out again (the rows in view
-- depend on how far the list is scrolled).
function Containers:settle(laid)
	local extent = laid.scroll_extent(LIST)
	if extent then
		self.list_view = extent.view
	end
	local moved = self.ui:settle(laid)
	if self.pointer then
		self:hover_to(laid, self.pointer)
	end
	return moved
end

-- --- The cursor --------------------------------------------------------------------------------

-- Where the cursor's row is among `rows` (nil when it is on none).
function Containers:cursor_at(rows)
	if not self.cursor then
		return nil
	end
	for index, row in ipairs(rows) do
		if row.key.id == self.cursor.id then
			return index
		end
	end
	return nil
end

-- After the rows changed: the cursor stays on its row if it is still there, else it goes to the
-- row nearest the place it had (`was`, a place among the rows before): the cursor is on a row
-- whenever there is one.
function Containers:rehome(was)
	local rows = self:rows()
	if self:cursor_at(rows) then
		return
	end
	-- With nothing chosen before (the filter found nothing, and then something), the first.
	local to = math.min(was or 1, #rows)
	local row = rows[to]
	self:set_cursor(row and row.key or nil)
end

-- Scrolls the least that shows the cursor's row.
function Containers:reveal_cursor()
	local rows = self:rows()
	local at = self:cursor_at(rows)
	if not at then
		return
	end
	local offset = self.ui:scroll_offset(LIST)
	local to = places(rows):reveal(at, offset, self.list_view)
	if to ~= offset then
		self.ui:set_scroll(LIST, to)
	end
end

-- --- The reply ---------------------------------------------------------------------------------

function Containers:reply(redraw)
	local hover = self.ui.hover
	local cursor = "arrow"
	if hover == SEARCH then
		-- The host's text field is over the filter's box: its own shape.
		cursor = "text"
	elseif hover ~= nil then
		cursor = "pointer"
	end
	return { redraw = redraw, cursor = cursor }
end

-- --- What the person does ----------------------------------------------------------------------

-- What the person did (`event`, as a page is told), with the page as it was last laid out (`laid`).
function Containers:input(event, laid)
	local kind = event.type
	if kind == "move" then
		local at = { x = event.x, y = event.y }
		self.pointer = at
		return self:reply(self:hover_to(laid, at))
	elseif kind == "leave" then
		self.pointer = nil
		return self:reply(self:hover_to(laid, nil))
	elseif kind == "press" then
		return self:press(laid, { x = event.x, y = event.y }, event.button, event.mods)
	elseif kind == "release" then
		if event.button == "left" then
			return self:release(laid, { x = event.x, y = event.y })
		end
		return self:reply(false)
	elseif kind == "scroll" then
		local at = { x = event.x, y = event.y }
		self.pointer = at
		return self:reply(self.ui:scroll_by(laid, at, event.dy))
	elseif kind == "key" then
		return self:key(event.key, event.mods, laid)
	end
	return self:reply(false)
end

function Containers:row_of(id)
	for _, row in ipairs(self:rows()) do
		if row.key.id == id then
			return row
		end
	end
	return nil
end

function Containers:press(laid, at, button, mods)
	self.pointer = at
	self.press_mods = mods
	local id = self.ui:press(laid, at)
	if id == nil then
		return self:reply(false)
	end
	if id == SEARCH then
		local reply = self:reply(false)
		reply.keyboard = "field"
		return reply
	end
	local row = self:row_of(id)
	if not row then
		return self:reply(false)
	end
	local changed = self:set_cursor(row.key)
	if button == "left" and (mods.command or mods.control or mods.shift) then
		-- ⌘-, ⌃- and ⇧-click mark, as the mouse's way to the space bar.
		local marked = self:toggle_mark(row)
		changed = changed or marked
	end
	if button == "right" then
		local reply = self:reply(changed)
		reply.menu = self:open_menu(at)
		return reply
	end
	return self:reply(changed)
end

function Containers:release(laid, at)
	local id = self.ui:release(laid, at)
	if id == nil then
		return self:reply(false)
	end
	local rows = self:rows()
	local was = self:cursor_at(rows)
	for _, status in ipairs(STATUSES) do
		if status_id(status) == id then
			local changed = self.status ~= status
			self.status = status
			self:rehome(was)
			return self:reply(changed)
		end
	end
	if id == GROUP_BUTTON then
		self.grouped = not self.grouped
		self:rehome(was)
		return self:reply(true)
	end
	local row = self:row_of(id)
	local mods = self.press_mods
	if row and row.what.group and row.what.project and not (mods.command or mods.control or mods.shift) then
		self:fold(row.what.project, not row.what.folded)
		return self:reply(true)
	end
	return self:reply(false)
end

function Containers:fold(project, folded)
	local kept = {}
	for _, name in ipairs(self.collapsed) do
		if name ~= project then
			kept[#kept + 1] = name
		end
	end
	if folded then
		kept[#kept + 1] = project
	end
	self.collapsed = kept
end

-- The names of the containers a row stands for.
function Containers:names_of(row)
	if row.what.group then
		local names = {}
		for index, at in ipairs(row.what.members) do
			names[index] = self.containers[at].name
		end
		return names
	end
	return { self.containers[row.what.container].name }
end

-- Marks the row's containers, or clears them when all are marked. Whether any mark changed.
function Containers:toggle_mark(row)
	local names = self:names_of(row)
	if #names == 0 then
		return false
	end
	local all = true
	for _, name in ipairs(names) do
		if not contains(self.marked, name) then
			all = false
			break
		end
	end
	if all then
		local kept = {}
		for _, name in ipairs(self.marked) do
			if not contains(names, name) then
				kept[#kept + 1] = name
			end
		end
		self.marked = kept
	else
		for _, name in ipairs(names) do
			if not contains(self.marked, name) then
				self.marked[#self.marked + 1] = name
			end
		end
	end
	return true
end

-- The command a key is, as the list's own keys have it.
local COMMANDS = { s = "stop", r = "restart", p = "pause", y = "copy_id", K = "kill", d = "remove" }

-- The keyboard's: moving the cursor, folding a project, marking, the filter, the commands. ⌃, ⌥
-- and ⌘ chords are the application's and are not read here.
function Containers:key(key, mods, laid)
	if mods.control or mods.alt or mods.command then
		return self:reply(false)
	end
	local rows = self:rows()
	local at = self:cursor_at(rows)
	local count = #rows
	local page = math.max(math.floor(self.list_view / ROW) - 1, 1)
	local function move_to(to)
		return math.min(to, count)
	end
	local target = nil
	if key == "down" or key == "j" then
		target = move_to(at and at + 1 or 1)
	elseif key == "up" or key == "k" then
		target = move_to(at and math.max(at - 1, 1) or 1)
	elseif key == "home" or key == "g" then
		target = move_to(1)
	elseif key == "end" or key == "G" then
		target = move_to(count)
	elseif key == "page_down" then
		target = move_to(at and at + page or 1)
	elseif key == "page_up" then
		target = move_to(at and math.max(at - page, 1) or 1)
	end
	if target then
		local row = rows[target]
		local changed = row ~= nil and self:set_cursor(row.key)
		self:reveal_cursor()
		return self:reply(changed)
	end
	if key == "escape" then
		return { close = true }
	elseif key == "tab" or key == "/" then
		local reply = self:reply(false)
		reply.keyboard = "field"
		return reply
	elseif key == "enter" then
		return self:fold_key(rows, at, nil)
	elseif key == "right" then
		return self:fold_key(rows, at, false)
	elseif key == "left" then
		local row = at and rows[at]
		if not row then
			return self:reply(false)
		end
		if row.what.group then
			return self:fold_key(rows, at, true)
		end
		-- From a container, back to the project it is under.
		local project = self.containers[row.what.container].project
		local changed = self.grouped and self:set_cursor(group_key(project))
		self:reveal_cursor()
		return self:reply(changed)
	elseif key == " " then
		local row = at and rows[at]
		if not row then
			return self:reply(false)
		end
		local changed = self:toggle_mark(row)
		-- Marking a container goes on to the next, to mark a run with the bar.
		local next_row = rows[at + 1]
		if not row.what.group and next_row then
			self:set_cursor(next_row.key)
			self:reveal_cursor()
		end
		return self:reply(changed or at ~= nil)
	elseif key == "a" then
		local marked = {}
		for _, row in ipairs(rows) do
			if not row.what.group then
				marked[#marked + 1] = self.containers[row.what.container].name
			end
		end
		self.marked = marked
		return self:reply(true)
	elseif key == "A" then
		local changed = #self.marked > 0
		self.marked = {}
		return self:reply(changed)
	elseif key == "f" then
		local place = 1
		for index, status in ipairs(STATUSES) do
			if status == self.status then
				place = index
			end
		end
		self.status = STATUSES[place % 4 + 1]
		self:rehome(at)
		self:reveal_cursor()
		return self:reply(true)
	elseif key == "P" then
		self.grouped = not self.grouped
		self:rehome(at)
		self:reveal_cursor()
		return self:reply(true)
	elseif key == "." then
		-- Under the row, a little in from its start: where the key's menu opens.
		local anchor = { x = 48, y = 120 }
		local bounds = self.cursor and laid.bounds(self.cursor.id)
		if bounds then
			anchor = { x = f32(bounds.x + 40), y = f32(f32(bounds.y + bounds.h) - 4) }
		end
		local reply = self:reply(false)
		reply.menu = self:open_menu(anchor)
		return reply
	end
	local act = COMMANDS[key]
	if act then
		return self:apply(act, self:targets(), at)
	end
	return self:reply(false)
end

-- ⏎ (nil: fold or unfold), → (false: unfold) and ← (true: fold) on a project's row.
function Containers:fold_key(rows, at, fold)
	local row = at and rows[at]
	if not (row and row.what.group and row.what.project) then
		return self:reply(false)
	end
	local to = fold
	if to == nil then
		to = not row.what.folded
	end
	if to == row.what.folded then
		return self:reply(false)
	end
	self:fold(row.what.project, to)
	self:reveal_cursor()
	return self:reply(true)
end

-- --- The filter --------------------------------------------------------------------------------

-- The text of the filter field changed.
function Containers:set_query(words)
	if self.query == words then
		return self:reply(false)
	end
	local rows = self:rows()
	local was = self:cursor_at(rows)
	self.query = words
	self:rehome(was)
	self:reveal_cursor()
	return self:reply(true)
end

-- --- Acting on containers ----------------------------------------------------------------------

-- The containers the commands and the menu act on: the marked ones, or else the cursor's row, a
-- container, or a project's containers the filter shows.
function Containers:targets()
	if #self.marked > 0 then
		local names = {}
		for _, c in ipairs(self.containers) do
			if contains(self.marked, c.name) then
				names[#names + 1] = c.name
			end
		end
		return names
	end
	local rows = self:rows()
	local at = self:cursor_at(rows)
	if not at then
		return {}
	end
	return self:names_of(rows[at])
end

function Containers:selected(names)
	local chosen = {}
	for _, c in ipairs(self.containers) do
		if contains(names, c.name) then
			chosen[#chosen + 1] = c
		end
	end
	return chosen
end

-- Runs `act` on the containers `names`: `stop` stops what runs and starts what does not, `pause`
-- pauses or resumes.
function Containers:apply(act, names, was)
	if #names == 0 then
		return self:reply(false)
	end
	local any_up, any_paused = false, false
	for _, c in ipairs(self:selected(names)) do
		any_up = any_up or is_up(c)
		any_paused = any_paused or c.state.kind == "paused"
	end
	-- The keys are one for two things.
	if act == "stop" and not any_up then
		act = "start"
	elseif act == "pause" and any_paused then
		act = "resume"
	end
	local copy = nil
	local removed = {}
	for _, c in ipairs(self.containers) do
		if contains(names, c.name) then
			if act == "start" and not is_up(c) then
				begin(c, "up 0s", "running")
			elseif act == "stop" and is_up(c) then
				finish(c, "now", 0)
			elseif act == "restart" then
				begin(c, "up 0s", "running")
			elseif act == "pause" and is_up(c) and c.state.kind ~= "paused" then
				c.state = { kind = "paused" }
			elseif act == "resume" and c.state.kind == "paused" then
				c.state = { kind = "running" }
			elseif act == "kill" and is_up(c) then
				finish(c, "now", 137)
			elseif act == "remove" then
				removed[#removed + 1] = c.name
			elseif act == "copy_id" then
				copy = container_id(c)
			end
		end
	end
	if #removed > 0 then
		local kept = {}
		for _, c in ipairs(self.containers) do
			if not contains(removed, c.name) then
				kept[#kept + 1] = c
			end
		end
		self.containers = kept
		local marked = {}
		for _, name in ipairs(self.marked) do
			if not contains(removed, name) then
				marked[#marked + 1] = name
			end
		end
		self.marked = marked
	end
	self:rehome(was)
	self:reveal_cursor()
	local reply = self:reply(true)
	reply.copy = copy
	return reply
end

-- The menu of the row the cursor is on (or of the marked containers): the entries, in the order
-- of the list's own keys; those that would need a program behind the list are shown and grey.
-- Nil where there is nothing to act on.
function Containers:open_menu(at)
	local targets = self:targets()
	if #targets == 0 then
		return nil
	end
	local selected = self:selected(targets)
	local single = #selected == 1
	local any_up, any_down, any_paused = false, false, false
	for _, c in ipairs(selected) do
		any_up = any_up or is_up(c)
		any_down = any_down or not is_up(c)
		any_paused = any_paused or c.state.kind == "paused"
	end
	local entries, acts = {}, {}
	local function item(title, key, act)
		entries[#entries + 1] = { title = title, key = key, enabled = act ~= nil }
		-- An entry that does nothing is false, so that the list of what they do has no holes.
		acts[#acts + 1] = act or false
	end
	if any_down then
		item("Start", "s", "start")
	end
	if any_up then
		item("Stop", "s", "stop")
	end
	item("Restart", "r", "restart")
	if any_paused then
		item("Resume", "p", "resume")
	elseif any_up then
		item("Pause", "p", "pause")
	end
	local function divide()
		if #entries > 0 and entries[#entries] ~= "-" then
			entries[#entries + 1] = "-"
			acts[#acts + 1] = false
		end
	end
	-- Without a program behind the list these have nothing to show: grey, but there, as in the
	-- tool this list is drawn from.
	divide()
	if single then
		for _, grey in ipairs({ { "Open a shell", "e" }, { "Logs", "l" }, { "Inspect", "i" } }) do
			entries[#entries + 1] = { title = grey[1], key = grey[2], enabled = false }
			acts[#acts + 1] = false
		end
		item("Copy id", "y", "copy_id")
	end
	divide()
	if any_up then
		item("Kill", "K", "kill")
	end
	item("Remove", "d", "remove")
	self.menu = { acts = acts, targets = targets }
	return { at = at, entries = entries }
end

-- The person picked entry `index` (from 1) of the menu that was opened.
function Containers:pick(index)
	local menu = self.menu
	self.menu = { acts = {}, targets = {} }
	local act = menu.acts[index]
	if not act then
		return self:reply(false)
	end
	local rows = self:rows()
	return self:apply(act, menu.targets, self:cursor_at(rows))
end

-- --- What a program tells the page --------------------------------------------------------------

-- Takes what the program reported: the line of each running container named moves on by a sample
-- and its numbers become the reading's. Whether the page changed.
function Containers:live(readings)
	local changed = false
	for _, reading in ipairs(readings) do
		local container = nil
		for _, c in ipairs(self.containers) do
			if is_up(c) and c.name == reading.name then
				container = c
				break
			end
		end
		if container then
			local cpu = f32(reading.cpu)
			cpu = finite(cpu) and math.clamp(cpu, 0, 10000) or 0
			local share = f32(reading.share)
			share = finite(share) and math.clamp(share, 0, 1) or 0
			-- At most this many characters (not bytes) of the memory text.
			local memory = reading.memory
			local ok, cut = pcall(utf8.offset, memory, MEMORY_TEXT + 1)
			if ok and cut then
				memory = string.sub(memory, 1, cut - 1)
			end
			if #container.history >= HISTORY then
				table.remove(container.history, 1)
			end
			container.history[#container.history + 1] = cpu
			container.cpu = cpu
			container.memory = { memory, share }
			changed = true
		end
	end
	return changed
end

-- The readings a `stats` push carries: `{"readings": [{"name", "cpu", "memory", "share"}, ..]}`.
-- What is not shaped so is left out.
local function readings_of(pushed)
	local list = type(pushed) == "table" and pushed.readings
	if type(list) ~= "table" then
		return {}
	end
	local readings = {}
	for _, one in ipairs(list) do
		if
			type(one) == "table"
			and type(one.name) == "string"
			and type(one.cpu) == "number"
			and type(one.memory) == "string"
			and type(one.share) == "number"
		then
			readings[#readings + 1] = { name = one.name, cpu = one.cpu, memory = one.memory, share = one.share }
			if #readings >= 256 then
				break
			end
		end
	end
	return readings
end

-- --- The payload -------------------------------------------------------------------------------

-- How much of the payload there is, in bytes of the data the chunks carry.
local UPLOAD_BYTES = 4 * 1024 * 1024
-- How much of it one message carries.
local CHUNK = 48 * 1024
-- The most chunks the page says in one call: a call may say 32 messages and is closed if it says
-- more, and the room the program has is not a reason to say them all at once.
local PER_CALL = 8
-- The number of the call that carries the last chunk: the program answers that one, with how
-- much it counted.
local LAST_CALL = (UPLOAD_BYTES + CHUNK - 1) // CHUNK

-- The payload on its way out.
local Upload = {}
Upload.__index = Upload

function Upload.new()
	return setmetatable({
		-- The number of the next chunk, from 1.
		next = 1,
		-- Bytes of data the program has been told of so far.
		told = 0,
		-- A chunk the program had no room for: the one to say first when there is.
		kept = nil,
		-- The program has no room: nothing is said until the page is called in `room`.
		waiting = false,
	}, Upload)
end

-- The next chunk and how much data it carries, if there is more. The last one says so.
function Upload:chunk(offered)
	if offered >= UPLOAD_BYTES then
		return nil
	end
	local size = math.min(CHUNK, UPLOAD_BYTES - offered)
	local last = offered + size >= UPLOAD_BYTES
	local call = self.next
	self.next += 1
	local params = { seq = call, data = string.rep("x", size), last = last }
	return { size = size, message = { call = { id = call, method = "upload", params = params } } }
end

-- Says what it can of the payload: stops at the call's own limit, and when the program has no
-- room. Whether there is more to say and the page may go on without being called.
function Upload:pump()
	local offered = self.told
	local said = 0
	while said < PER_CALL do
		local chunk = self.kept
		self.kept = nil
		if not chunk then
			chunk = self:chunk(offered)
			if not chunk then
				return false
			end
		end
		if bateri.send(chunk.message) then
			self.told += chunk.size
			offered = self.told
			said += 1
		else
			self.kept = chunk
			self.waiting = true
			return false
		end
	end
	return self.told < UPLOAD_BYTES
end

-- How much of the payload the program has been told, in percent.
function Upload:percent()
	return self.told * 100 // UPLOAD_BYTES
end

-- --- The page ----------------------------------------------------------------------------------

local list = Containers.fixture()
local upload = nil
-- What the program counted of the last payload, once it has said.
local counted = nil
-- There is more of the payload to say and nothing but the next frame calls the page.
local more = false
-- Seconds without a reading after which the page says none is coming: the program pushes one a
-- second, so five missed is a program that stopped, not one that is late.
local STALE_AFTER = 5
-- A reading came since the page was last rendered; when the newest came; none has for a while.
local fresh = false
local last_reading = nil
local stale = false

-- The program counted the payload: it says how much it got.
local function on_last(result)
	counted = nil
	if type(result) == "table" and type(result.bytes) == "number" and result.bytes >= 0 and result.bytes % 1 == 0 then
		counted = result.bytes
	end
	upload = nil
	more = false
	return true
end

-- Says the payload on, as far as the program has the room.
local function pump_upload()
	if not upload then
		more = false
		return
	end
	more = upload:pump()
end

-- What the page says over the list, if it has anything to say: how far the payload has got while
-- the program is being told, that the readings stopped, what the program counted of the payload
-- when it has said.
local function note()
	if upload and upload.waiting then
		return string.format("Sending 4 MiB to demopen... %d%% (it has not read the last yet)", upload:percent())
	elseif upload then
		return string.format("Sending 4 MiB to demopen... %d%%", upload:percent())
	elseif stale then
		return "No new readings from demopen"
	elseif counted then
		return string.format("Sent 4 MiB - demopen counted %d bytes", counted)
	end
	return nil
end

-- The program pushes the numbers of the running containers once a second. Every reading is a
-- frame, even one that changes nothing on the list: it is when the page learns that readings are
-- still coming.
bateri.subscribe("stats", nil, function(pushed)
	list:live(readings_of(pushed))
	fresh = true
	return true
end)

return {
	render = function(size, now, reduce_motion)
		if more then
			pump_upload()
		end
		-- The page learns the time only here: a reading that came is stamped with the frame it
		-- asked for, and the page asks to be rendered once more when the next one is late.
		if fresh then
			fresh = false
			last_reading = now
		end
		local stale_at = last_reading and last_reading + STALE_AFTER
		stale = stale_at ~= nil and now >= stale_at
		list:advance(now, reduce_motion)
		local view = list:view(size)
		local words = note()
		-- The payload says how it is doing over the list, above its status line.
		local root = view
		if words then
			root = div():size_full():child(view):child(
				div():absolute():inset(nil, 12, 36, nil):px(12):py(6):rounded(6):bg("raised2"):border(1, "line")
					:text_size(12.5):color("foreground"):child(text(words))
			)
		end
		local wake_at = nil
		if not stale then
			wake_at = stale_at
		end
		return root, list:animating() or more, wake_at
	end,

	settle = function()
		return list:settle(bateri.layout)
	end,

	handle = function(event)
		local kind = event.type
		if kind == "key" and event.key == "U" and not (event.mods.control or event.mods.alt or event.mods.command) then
			-- A page nobody opened has no one to send a payload to.
			if upload == nil and bateri.heard() then
				upload = Upload.new()
				counted = nil
				pump_upload()
			end
			return true
		elseif kind == "text" then
			return list:set_query(event.text)
		elseif kind == "menu" then
			return list:pick(event.index)
		end
		return list:input(event, bateri.layout)
	end,

	room = function()
		if upload then
			upload.waiting = false
		end
		pump_upload()
		return true
	end,

	-- The program answers the last chunk of the payload with how much it counted; the rest of what
	-- it says (a response to nothing the page asked, a message that is not one) shows nothing.
	message = function(message)
		local response = message.response
		if type(response) == "table" and response.id == LAST_CALL and upload then
			return on_last(response.result)
		end
		return nil
	end,
}
