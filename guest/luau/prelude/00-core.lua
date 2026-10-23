-- The page API a script is written against, part 1: what the host gives (kept out of the
-- script's reach once it is wrapped) and the small things the rest is made of.
--
-- A script is Luau. What it can use besides the language is the base functions, `string`,
-- `table`, `math` and `utf8`, and `bateri`, the table this prelude fills:
--
--   elements   bateri.div(), bateri.text(), bateri.spark(), bateri.spacer()
--   colours    a tone's name ("accent", "raised2", ...) or bateri.alpha("accent", 0.5)
--   pages      the script returns { render = ..., handle = ..., settle = ..., message = ..., room = ... }
--   memory     bateri.Ui, bateri.Fades, bateri.Rows (what a page remembers between frames)
--   layout     bateri.layout (what is where on the page as it was last laid out)
--   program    bateri.send, bateri.call, bateri.subscribe, bateri.cancel, bateri.json
--
-- Lengths are points. An element is a plain table; the builders are methods that return it.

bateri.version = 1

-- A number as the engine holds it, in single precision. The engine measures and draws in
-- single precision, and a page that adds up highlights or sizes in doubles can come to a
-- value that is not the engine's (a level of 1e-17 where the engine has 0).
function bateri.f32(value)
	return (string.unpack("<f", string.pack("<f", value)))
end

-- The host's functions, taken here and given a shape of Lua's below (`bateri.layout`,
-- `bateri.send`); the table of the raw ones stays at `bateri.host` for a page that wants them.
local host = {
	send = bateri.send,
	hit = bateri.hit,
	ids_at = bateri.ids_at,
	bounds = bateri.bounds,
	scroll_extent = bateri.scroll_extent,
	focus_order = bateri.focus_order,
	path_of = bateri.path_of,
	text_field = bateri.text_field,
}
bateri.send, bateri.hit, bateri.ids_at, bateri.bounds = nil, nil, nil, nil
bateri.scroll_extent, bateri.focus_order, bateri.path_of, bateri.text_field = nil, nil, nil, nil
bateri.host = host

-- A colour: a tone with an opacity. Everywhere a colour is taken, a tone's name does too.
function bateri.alpha(tone, alpha)
	return { tone, alpha }
end

-- The ground raised by `steps` levels ("raised1" is a card on the pane).
function bateri.raised(steps)
	return "raised" .. steps
end

-- A grid track: points are a number, the rest are these.
function bateri.fr(share)
	return { fr = share }
end

function bateri.range(least, share)
	return { min = least, fr = share }
end

-- What the host answers about the page as it laid it out last (which may be a frame behind
-- the tree handed out since). Points are `{ x =, y = }`.
local layout = {}
bateri.layout = layout

function layout.hit(at)
	return host.hit(at.x, at.y)
end

function layout.ids_at(at)
	return host.ids_at(at.x, at.y)
end

function layout.bounds(id)
	return host.bounds(id)
end

-- How far the scrolling box named `id` reaches: { content =, view = }, with `max_offset`.
function layout.scroll_extent(id)
	local extent = host.scroll_extent(id)
	if extent then
		extent.max_offset = math.max(extent.content - extent.view, 0)
	end
	return extent
end

function layout.focus_order()
	return host.focus_order()
end

function layout.path_of(id)
	return host.path_of(id)
end

function layout.text_field()
	return host.text_field()
end
