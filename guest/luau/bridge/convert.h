// What crosses between the Lua page and the interface: the tree a script hands out becomes the
// interface's list of nodes, what the person did becomes a table, and the table a script
// answers with becomes the interface's reply.
//
// Everything here runs on the Lua stack of a `lua_pcall` (a script's mistake in its tree is a
// Lua error that names it), and everything it allocates for the host is `malloc`ed, because the
// generated `_free` functions are what releases it once the host has read it.

#pragma once

#include <stdint.h>
#include <string>

#include "lua.h"

extern "C" {
#include "view.h"
}

namespace bridge {

// A string for the host: valid UTF-8 (a byte that is not part of a sequence becomes U+FFFD),
// since a string that is not would be a trap in the host's reading of it.
view_string_t host_string(const char* bytes, size_t length);
std::string repaired(const char* bytes, size_t length);

// The 64-bit name of an element. A string is the FNV-1a hash of its bytes, the number the
// engine's `Id::named` makes of the same words; a number is the id with that number.
uint64_t id_of(lua_State* L, int index);
// The script's own name for an id the host answers with: the string or number it used. An id
// the page never named is its number (or its hex digits where a number would not hold it).
void push_id(lua_State* L, uint64_t id);
// Forget the names handed out so far (the table of them is bounded by the pages' own names).
void forget_ids();

// The tree at `root` as nodes. Run it as a protected C function (`frame_call`).
struct FrameJob {
    bateri_view_types_frame_t* out;
    bool animating;
    // When the page at rest wants to be rendered again (`has_wake_at`), on render's clock.
    bool has_wake_at;
    double wake_at;
};
int frame_call(lua_State* L);  // (root element, FrameJob*) -> nothing

// The event as a table on top of the stack.
void push_event(lua_State* L, const bateri_view_types_event_t& event);

// The value at the stack index `reply` (nil, a boolean, or a table) as a reply.
struct ReplyJob {
    bateri_view_types_reply_t* out;
};
int reply_call(lua_State* L);  // (reply value, ReplyJob*) -> nothing

}  // namespace bridge
