-- The page API, part 5: the page itself and its program.
--
-- A script returns the page, a table of the functions the application calls:
--
--   render(size, now, reduce_motion) -> element[, animating[, wake_at]]
--       The page for a viewport `size` ({ w =, h = } points) at time `now` (seconds on the
--       application's clock). `reduce_motion` is the person's setting. `animating` true asks
--       for the next frame at once; a page at rest costs no frames. `wake_at`, a time on the
--       same clock (`now + 5`: in five seconds), asks a page at rest to be rendered again once
--       then, if nothing does before: what a page needs to show that what it waits for has not
--       come (a reading that stopped).
--   settle() -> boolean            (optional)
--       The page was laid out: bring what depends on the layout back within it. True when
--       something changed and the page should be rendered again.
--   handle(event) -> reply         (optional)
--       The person did something. `event` is { type = "move" | "leave" | "press" | "release" |
--       "scroll" | "key" | "text" | "menu", ... } in the page's points: move/press/release/
--       scroll have x and y (press and release a button, "left" or "right"; press and key
--       mods = { shift =, control =, alt =, command = }; scroll dy, the points the content
--       moves up by), key has `key` ("up", "down", "left", "right", "page_up", "page_down",
--       "home", "end", "enter", "escape", "tab", or the character), text has the text of the
--       application's field, menu has the `index` (from 1) picked of the menu the page asked for.
--   message(message) -> reply      (optional)
--       The program said something that is not an answer to a call the page made or a push of a
--       subscription it made (those go to the functions given to `bateri.call` and
--       `bateri.subscribe`): a response or a push to a number the page did not ask with, or
--       anything else. `message` is the decoded JSON.
--   room() -> reply                (optional)
--       The program has read enough: what `bateri.send` answered false to may be sent again.
--
-- A reply is nil (nothing changed), true or false (a frame is worth drawing or not) or a table:
--   { redraw =, close =, copy = "text for the pasteboard", cursor = "arrow" | "pointer" | "text",
--     keyboard = "page" | "field",
--     menu = { at = { x =, y = }, entries = { { title =, key =, enabled = }, "-" (a line), ... } } }
--
-- What the page says to its program is JSON objects, the six messages of a view and its program:
-- `bateri.call` asks the program to do something and answers once, `bateri.subscribe` asks to hear
-- a topic as it changes, `bateri.cancel` gives either up. The program may not have read what it was
-- told before: what is said waits its turn and goes when the program can take it (`bateri.send`,
-- the raw word, says whether it took the message, and the page keeps what it was not).

local host = bateri.host
local json = bateri.json

-- A call may say this many messages; the application closes a page that says more.
local PER_CALL = 8

local page = nil
-- The page has been rendered: it has been laid out only after that.
local rendered = false
-- What is waiting to be said, in order: `head` is the next, `tail` the last. Counted, not
-- measured with `#`: the queue is emptied from the front, and the length of a table with holes is
-- any of its borders.
local queue, head, tail = {}, 1, 0
-- The program had no room for the next message: nothing is said until it has (`room`).
local full = false
-- The program has said something.
local heard = false
local next_id = 1
-- What is asked and not yet over, by the number the page gave it.
local asked = {}

-- Says `message` (a table) to the program now. True when the program took it; false when it
-- has not the room (nothing was sent: keep it, `room` says when to try again).
function bateri.send(message)
	return host.send(json.encode(message))
end

-- Whether the program has said something: a page nobody opened has no one to talk to.
function bateri.heard()
	return heard
end

local function enqueue(message)
	tail += 1
	queue[tail] = json.encode(message)
end

-- Says what is waiting, as far as the program has the room and a call may say. True when there
-- is more that only a later call will say.
local function flush()
	local said = 0
	while head <= tail do
		if full then
			return false
		end
		if said >= PER_CALL then
			return true
		end
		if host.send(queue[head]) then
			queue[head] = nil
			head += 1
			said += 1
		else
			full = true
			return false
		end
	end
	queue, head, tail = {}, 1, 0
	return false
end

-- `reply`, asking for the next frame as well: there is more to say than one call may, and a
-- render says it on (and asks for the frame after it while there is more still).
local function with_frame(reply)
	if type(reply) == "table" then
		reply.redraw = true
		return reply
	end
	return true
end

-- Asks the program to do `method` (with `params`, a value) and answer once: `done(result, err)`
-- is called with the result, or with nil and { code =, message = }. `progress(report)` is called
-- with { done =, total =, note = } while a call that takes a while goes on. The number of the call.
function bateri.call(method, params, done, progress)
	local id = next_id
	next_id += 1
	asked[id] = { kind = "call", done = done, progress = progress }
	enqueue({ call = { id = id, method = method, params = params } })
	return id
end

-- Asks to hear `topic` as it changes: `on_value(value)` is called for each value the program
-- pushes. The number of the subscription.
function bateri.subscribe(topic, params, on_value)
	local id = next_id
	next_id += 1
	asked[id] = { kind = "subscribe", on_value = on_value }
	enqueue({ subscribe = { id = id, topic = topic, params = params } })
	return id
end

-- Gives up a call that is still out, or a subscription.
function bateri.cancel(id)
	asked[id] = nil
	enqueue({ cancel = { id = id } })
end

-- --- What the application calls ----------------------------------------------------------------

-- The functions below are the application's entry points (`bateri.start` .. `bateri.room`),
-- called by the interpreter's bridge; the script does not call them.

function bateri.start(returned)
	if type(returned) ~= "table" or type(returned.render) ~= "function" then
		error("the script must return a page: a table with a render function", 0)
	end
	page = returned
end

function bateri.render(width, height, now, reduce_motion)
	local root, animating, wake_at = page.render({ w = width, h = height }, now, reduce_motion)
	if type(root) ~= "table" then
		error("render must return an element, not a " .. type(root), 0)
	end
	if wake_at ~= nil and type(wake_at) ~= "number" then
		error("render's third value, when to render again, must be a time (a number), not a " .. type(wake_at), 0)
	end
	rendered = true
	local more = flush()
	return root, (animating or more) and true or false, wake_at
end

function bateri.settle()
	-- Before the first page there is no layout to settle to: the application asks once as the
	-- script starts, and that is a call that only starts it.
	if rendered and page.settle then
		return page.settle() and true or false
	end
	return false
end

function bateri.handle(event)
	local reply = nil
	if page.handle then
		reply = page.handle(event)
	end
	if flush() then
		reply = with_frame(reply)
	end
	return reply
end

-- A message of the program: the answer to what the page asked is for the one who asked, the rest
-- for the page.
function bateri.message(text)
	heard = true
	local message = json.decode(text)
	if type(message) ~= "table" then
		return nil
	end
	local reply = nil
	local taken = false
	-- What the page asked is found by the number it gave and by what it asked: an answer is for a
	-- call, a push for a subscription.
	local function asked_as(body, kind)
		local request = type(body) == "table" and type(body.id) == "number" and asked[body.id]
		if request and request.kind == kind then
			return request
		end
		return nil
	end
	if message.push then
		local request = asked_as(message.push, "subscribe")
		if request then
			taken = true
			if request.on_value then
				reply = request.on_value(message.push.value)
			end
		end
	elseif message.response then
		local request = asked_as(message.response, "call")
		if request then
			taken = true
			asked[message.response.id] = nil
			if request.done then
				reply = request.done(message.response.result, message.response.error)
			end
		end
	elseif message.progress then
		local request = asked_as(message.progress, "call")
		if request then
			taken = true
			if request.progress then
				reply = request.progress(message.progress)
			end
		end
	end
	if not taken and page.message then
		reply = page.message(message)
	end
	if flush() then
		reply = with_frame(reply)
	end
	return reply
end

function bateri.room()
	full = false
	flush()
	local reply = nil
	if page.room then
		reply = page.room()
	end
	if flush() then
		reply = with_frame(reply)
	end
	return reply
end
