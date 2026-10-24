// The Lua page: Luau, the page interface (`crates/bt-view-wasm/wit/view.wit`) and a thin layer
// between them.
//
// This file is the interface's side of the page: it implements what a page exports (render,
// settle, handle, message, room), gives the script what it may ask of the application (the
// layout, a way to speak to the program) and says why when the script cannot go on. What a page
// *is* — its elements, the pointer's bookkeeping, the messages with the program — is Lua, in
// `prelude/`, built into this module; a script's own text is what the application hands over
// through `script.source`.
//
// The script runs in Luau's sandbox, with the pure libraries only (`string`, `table`, `math`,
// `utf8` and the base functions): no `io`, `os` or `debug`, no way to load bytecode, no
// `require`. The module itself can reach nothing the page interface does not give it; the
// calls the C library would make to the system are answered here (see the end of this file).

#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include <string>

#include "convert.h"
#include "lua.h"
#include "luacode.h"
#include "lualib.h"

extern "C" {
#include "view.h"
}

// The preludes, as `kPrelude[i]` (a name and the source), written by the build script.
#include "prelude.inc"

namespace {

lua_State* L = nullptr;
// The state is made when the page is first asked for something. The application asks for
// `settle` once as the module starts (before any page, so that it is a call that only starts
// the script): a script that cannot start fails there, under the time a module has to start in,
// and the first page is not charged for it.
bool started = false;
// The script cannot go on: the application has been told, and is closing the page.
bool broken = false;

constexpr size_t kMessageLimit = 2048;

void fail(const std::string& why) {
    broken = true;
    std::string text = bridge::repaired(why.data(), why.size() > kMessageLimit ? kMessageLimit : why.size());
    view_string_t message;
    message.ptr = (uint8_t*)text.data();
    message.len = text.size();
    bateri_view_diagnostics_fail(&message);
}

// The error on top of the stack as words, popped.
std::string reason(lua_State* state) {
    std::string out;
    if (lua_type(state, -1) == LUA_TSTRING) {
        out = lua_tostring(state, -1);
    } else {
        size_t length = 0;
        const char* shown = luaL_tolstring(state, -1, &length);
        out.assign(shown, length);
        lua_pop(state, 1);
    }
    lua_pop(state, 1);
    return out;
}

// Compiles `source` as the chunk `name` and leaves it on the stack of `state`; the error
// otherwise (the line is in it).
bool load(lua_State* state, const char* name, const char* source, size_t length, std::string& error) {
    lua_CompileOptions options = {};
    options.optimizationLevel = 1;
    options.debugLevel = 1;
    size_t size = 0;
    char* bytecode = luau_compile(source, length, &options, &size);
    int status = luau_load(state, name, bytecode, size, 0);
    free(bytecode);
    if (status != 0) {
        error = reason(state);
        return false;
    }
    return true;
}

// --- What the script may ask of the application -------------------------------------------------

int l_send(lua_State* state) {
    size_t length = 0;
    const char* bytes = luaL_checklstring(state, 1, &length);
    std::string text = bridge::repaired(bytes, length);
    view_string_t message;
    message.ptr = (uint8_t*)text.data();
    message.len = text.size();
    lua_pushboolean(state, bateri_view_program_send(&message) == BATERI_VIEW_PROGRAM_DELIVERY_ACCEPTED);
    return 1;
}

void push_rect(lua_State* state, const bateri_view_types_rect_t& rect) {
    lua_createtable(state, 0, 4);
    lua_pushnumber(state, rect.x);
    lua_setfield(state, -2, "x");
    lua_pushnumber(state, rect.y);
    lua_setfield(state, -2, "y");
    lua_pushnumber(state, rect.w);
    lua_setfield(state, -2, "w");
    lua_pushnumber(state, rect.h);
    lua_setfield(state, -2, "h");
}

void push_ids(lua_State* state, view_list_id_t& ids) {
    lua_createtable(state, (int)ids.len, 0);
    for (size_t at = 0; at < ids.len; at++) {
        bridge::push_id(state, ids.ptr[at]);
        lua_rawseti(state, -2, (int)at + 1);
    }
    view_list_id_free(&ids);
}

int l_hit(lua_State* state) {
    bateri_view_types_point_t at = {(float)luaL_checknumber(state, 1), (float)luaL_checknumber(state, 2)};
    uint64_t id = 0;
    if (bateri_view_layout_hit(&at, &id)) bridge::push_id(state, id);
    else lua_pushnil(state);
    return 1;
}

int l_ids_at(lua_State* state) {
    bateri_view_types_point_t at = {(float)luaL_checknumber(state, 1), (float)luaL_checknumber(state, 2)};
    view_list_id_t ids = {nullptr, 0};
    bateri_view_layout_ids_at(&at, &ids);
    push_ids(state, ids);
    return 1;
}

int l_bounds(lua_State* state) {
    bateri_view_types_rect_t rect;
    if (bateri_view_layout_bounds(bridge::id_of(state, 1), &rect)) push_rect(state, rect);
    else lua_pushnil(state);
    return 1;
}

int l_scroll_extent(lua_State* state) {
    bateri_view_types_extent_t extent;
    if (bateri_view_layout_scroll_extent(bridge::id_of(state, 1), &extent)) {
        lua_createtable(state, 0, 2);
        lua_pushnumber(state, extent.content);
        lua_setfield(state, -2, "content");
        lua_pushnumber(state, extent.view);
        lua_setfield(state, -2, "view");
    } else {
        lua_pushnil(state);
    }
    return 1;
}

int l_focus_order(lua_State* state) {
    view_list_id_t ids = {nullptr, 0};
    bateri_view_layout_focus_order(&ids);
    push_ids(state, ids);
    return 1;
}

int l_path_of(lua_State* state) {
    view_list_id_t ids = {nullptr, 0};
    bateri_view_layout_path_of(bridge::id_of(state, 1), &ids);
    push_ids(state, ids);
    return 1;
}

int l_text_field(lua_State* state) {
    bateri_view_types_rect_t rect;
    if (bateri_view_layout_text_field(&rect)) push_rect(state, rect);
    else lua_pushnil(state);
    return 1;
}

// --- Starting ------------------------------------------------------------------------------------

void open_libraries(lua_State* state) {
    luaopen_base(state);
    lua_pop(state, 1);
    luaopen_table(state);
    lua_pop(state, 1);
    luaopen_string(state);
    lua_pop(state, 1);
    luaopen_math(state);
    lua_pop(state, 1);
    luaopen_utf8(state);
    lua_pop(state, 1);
    // The page has nowhere to print to, and nothing of the system to ask: what the base
    // library has of either goes.
    static const char* const gone[] = {"print", "collectgarbage", "gcinfo", "newproxy", "getfenv", "setfenv"};
    for (const char* name : gone) {
        lua_pushnil(state);
        lua_setglobal(state, name);
    }
}

void host_functions(lua_State* state) {
    static const luaL_Reg functions[] = {
        {"send", l_send},
        {"hit", l_hit},
        {"ids_at", l_ids_at},
        {"bounds", l_bounds},
        {"scroll_extent", l_scroll_extent},
        {"focus_order", l_focus_order},
        {"path_of", l_path_of},
        {"text_field", l_text_field},
        {nullptr, nullptr},
    };
    lua_newtable(state);
    luaL_register(state, nullptr, functions);
    lua_setglobal(state, "bateri");
}

// Makes the state: the libraries, the prelude, the sandbox, then the script. False (and the
// application told why) when any of it fails.
bool start() {
    started = true;
    L = luaL_newstate();
    if (!L) {
        fail("the interpreter could not be started");
        return false;
    }
    open_libraries(L);
    host_functions(L);
    for (const auto& chunk : kPrelude) {
        std::string error;
        std::string name = std::string("=") + chunk.name;
        if (!load(L, name.c_str(), chunk.source, chunk.length, error) || lua_pcall(L, 0, 0, 0) != 0) {
            if (error.empty()) error = reason(L);
            fail(error);
            return false;
        }
    }
    luaL_sandbox(L);
    // The script gets a thread of its own, with globals of its own over the shared, read-only
    // ones: what it assigns is its own.
    lua_State* thread = lua_newthread(L);
    lua_ref(L, -1);
    lua_pop(L, 1);
    luaL_sandboxthread(thread);

    view_list_u8_t source = {nullptr, 0};
    bateri_view_script_source(&source);
    std::string error;
    bool loaded = load(thread, "=script", (const char*)source.ptr, source.len, error);
    view_list_u8_free(&source);
    if (!loaded) {
        fail(error);
        return false;
    }
    if (lua_pcall(thread, 0, 1, 0) != 0) {
        fail(reason(thread));
        return false;
    }
    // The prelude's `start` takes what the script returned: the page.
    lua_getglobal(L, "bateri");
    lua_getfield(L, -1, "start");
    lua_xmove(thread, L, 1);
    if (lua_pcall(L, 1, 0, 0) != 0) {
        fail(reason(L));
        return false;
    }
    lua_pop(L, 1);
    return true;
}

bool ready() {
    if (broken) return false;
    if (started) return true;
    return start();
}

// Calls `bateri.<name>` with the `arguments` the caller has pushed after it, protected. The
// results are left on the stack; false when it failed (and the application has been told).
bool entry(const char* name, int arguments, int results) {
    // The function goes below its arguments.
    lua_getglobal(L, "bateri");
    lua_getfield(L, -1, name);
    lua_remove(L, -2);
    lua_insert(L, -(arguments + 1));
    if (lua_pcall(L, arguments, results, 0) != 0) {
        fail(reason(L));
        return false;
    }
    return true;
}

// The value on top of the stack as a reply, popped; the default where it is wrong.
void answer(bateri_view_types_reply_t* out) {
    bridge::ReplyJob job = {out};
    lua_pushcfunction(L, bridge::reply_call, "reply");
    lua_insert(L, -2);
    lua_pushlightuserdata(L, &job);
    if (lua_pcall(L, 2, 0, 0) != 0) {
        bateri_view_types_reply_free(out);
        memset(out, 0, sizeof *out);
        fail(reason(L));
    }
}

}  // namespace

// --- What a page exports -------------------------------------------------------------------------

void exports_bateri_view_page_render(exports_bateri_view_page_size_t* size, double now, bool reduce_motion,
                                     exports_bateri_view_page_frame_t* ret) {
    memset(ret, 0, sizeof *ret);
    if (!ready()) return;
    lua_pushnumber(L, size->w);
    lua_pushnumber(L, size->h);
    lua_pushnumber(L, now);
    lua_pushboolean(L, reduce_motion);
    if (!entry("render", 4, 3)) return;
    // The prelude hands back the element, whether it moves and when it wants to be asked again
    // (a number or nil: it checked).
    bool has_wake_at = lua_type(L, -1) == LUA_TNUMBER;
    double wake_at = has_wake_at ? lua_tonumber(L, -1) : 0.0;
    bridge::FrameJob job = {ret, lua_toboolean(L, -2) != 0, has_wake_at, wake_at};
    lua_pop(L, 2);
    lua_pushcfunction(L, bridge::frame_call, "frame");
    lua_insert(L, -2);
    lua_pushlightuserdata(L, &job);
    if (lua_pcall(L, 2, 0, 0) != 0) {
        exports_bateri_view_page_frame_free(ret);
        memset(ret, 0, sizeof *ret);
        fail(reason(L));
    }
}

bool exports_bateri_view_page_settle(void) {
    if (!ready()) return false;
    if (!entry("settle", 0, 1)) return false;
    bool changed = lua_toboolean(L, -1) != 0;
    lua_pop(L, 1);
    return changed;
}

void exports_bateri_view_page_handle(exports_bateri_view_page_event_t* event,
                                     exports_bateri_view_page_reply_t* ret) {
    memset(ret, 0, sizeof *ret);
    if (ready()) {
        bridge::push_event(L, *event);
        if (entry("handle", 1, 1)) answer(ret);
    }
    exports_bateri_view_page_event_free(event);
}

void exports_bateri_view_page_message(view_string_t* message, exports_bateri_view_page_reply_t* ret) {
    memset(ret, 0, sizeof *ret);
    if (ready()) {
        lua_pushlstring(L, (const char*)message->ptr, message->len);
        if (entry("message", 1, 1)) answer(ret);
    }
    view_string_free(message);
}

void exports_bateri_view_page_room(exports_bateri_view_page_reply_t* ret) {
    memset(ret, 0, sizeof *ret);
    if (!ready()) return;
    if (entry("room", 0, 1)) answer(ret);
}

// --- What the system is not asked --------------------------------------------------------------
//
// The C library asks the system for a few things (the clock, the environment, a place to write,
// a way to exit). The module has no system to ask, so the questions are answered here, where
// they stop: nothing is written anywhere, there is no environment, the clock reads zero (the
// page is told the time as an argument of `render`), and an exit is a trap. Defining the names
// the library asks by is what keeps them from becoming imports of the component.

extern "C" {

clock_t clock(void) { return 0; }

uint16_t __imported_wasi_snapshot_preview1_environ_sizes_get(uint32_t* count, uint32_t* bytes) {
    *count = 0;
    *bytes = 0;
    return 0;
}

uint16_t __imported_wasi_snapshot_preview1_environ_get(uint8_t** environ, uint8_t* buffer) {
    (void)environ;
    (void)buffer;
    return 0;
}

uint16_t __imported_wasi_snapshot_preview1_clock_time_get(uint32_t clock, uint64_t precision, uint64_t* time) {
    (void)clock;
    (void)precision;
    *time = 0;
    return 0;
}

// 8 is "bad file descriptor": there is no file to close, to seek in or to write to.
uint16_t __imported_wasi_snapshot_preview1_fd_close(uint32_t fd) {
    (void)fd;
    return 8;
}

uint16_t __imported_wasi_snapshot_preview1_fd_fdstat_get(uint32_t fd, void* stat) {
    (void)fd;
    (void)stat;
    return 8;
}

uint16_t __imported_wasi_snapshot_preview1_fd_seek(uint32_t fd, int64_t offset, uint8_t whence, uint64_t* position) {
    (void)fd;
    (void)offset;
    (void)whence;
    (void)position;
    return 8;
}

uint16_t __imported_wasi_snapshot_preview1_fd_write(uint32_t fd, const void* vectors, uint32_t count,
                                                    uint32_t* written) {
    (void)fd;
    (void)vectors;
    (void)count;
    *written = 0;
    return 8;
}

_Noreturn void __imported_wasi_snapshot_preview1_proc_exit(uint32_t code) {
    (void)code;
    __builtin_trap();
}

}  // extern "C"
