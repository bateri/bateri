/*
 * bt_embed.h — bateri's terminal pane for an application not written in Rust.
 *
 * Link libbt_embed.a (built by `cargo build -p bt-embed`) with the system
 * frameworks `make embed-swift` lists. The header is written by hand beside
 * crates/bt-embed/src/lib.rs; the two change together, and a test checks that
 * every function and constant here is the library's.
 *
 * The contract:
 *  - A published function never changes its signature or meaning. A new need
 *    is a new function; an old one stays at least one release, marked in the
 *    changelog as going. Nothing crosses as a struct that could grow: a
 *    configuration is built with setters, an event is read with getters.
 *  - Every call is made on the main thread. Off it, a call does nothing and
 *    returns its failure value. The configuration's setters, the event's
 *    getters and bt_string_free touch no AppKit and may be called anywhere.
 *  - A `char *` this library returns is the caller's, freed with
 *    bt_string_free. A `const char *` an event gives is lent for the
 *    handler's call only.
 *  - A configuration is consumed by bt_pane_open (or freed with
 *    bt_pane_config_free); a pane handle is freed by bt_pane_close.
 *
 * A pane follows its window by itself: its focus with the window's key state,
 * its drawing with the window's occlusion, its sharpness with the screen's
 * scale. What no notification tells a view — the host hiding the pane or a
 * view above it — the host says (bt_pane_set_hidden,
 * bt_pane_visibility_changed). A question the pane asks (a confirmation, a
 * password) is a sheet on the pane's window.
 */

#ifndef BT_EMBED_H
#define BT_EMBED_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* The interface's version. A host checks it once, before anything else. */
#define BT_EMBED_ABI_VERSION 1u
uint32_t bt_embed_abi_version(void);

/* Frees a string this library returned. NULL is ignored. */
void bt_string_free(char *text);

/* ---- Events ------------------------------------------------------------ */

typedef struct BtEvent BtEvent;

/* Called on the main thread with the context given at
 * bt_pane_config_set_event_handler and the event, lent for the call. The
 * handler may call back into this library, bt_pane_close included. */
typedef void (*BtEventHandler)(void *context, const BtEvent *event);

/* The kinds bt_event_kind answers; what each fills is said beside it. */
/* The title, working directory or remote state changed: read them again. */
#define BT_EVENT_TITLE 1u
/* The shell exited: the pane has nothing left to show; the host closes it. */
#define BT_EVENT_SHELL_EXITED 2u
/* The keyboard arrived at the pane. */
#define BT_EVENT_FOCUSED 3u
/* A command started or ended, or the alternate screen came or went. */
#define BT_EVENT_ACTIVITY 4u
/* A remote transfer's progress or existence changed. */
#define BT_EVENT_UPLOADS 5u
/* A notification for the user: text = title, detail = body. */
#define BT_EVENT_NOTIFY 6u
/* Notices about the pane's setup, one per line in text; number = source. */
#define BT_EVENT_NOTICES 7u
/* Files are dragged over the pane (flag) or no longer are. */
#define BT_EVENT_FILES_DRAGGED 8u
/* A press with Option-Command landed on the pane at x/y (window points). */
#define BT_EVENT_CARRY_PRESS 9u
/* A question of the pane was put off or taken up. */
#define BT_EVENT_QUESTIONS 10u

/* A notice's source (BT_EVENT_NOTICES' number). */
#define BT_NOTICE_SOURCE_WRITE 0
#define BT_NOTICE_SOURCE_SETTINGS 1
#define BT_NOTICE_SOURCE_THEME 2
#define BT_NOTICE_SOURCE_FONT 3

/* The event's kind; 0 for NULL. */
uint32_t bt_event_kind(const BtEvent *event);
/* The id of the pane the event is about. */
uint64_t bt_event_pane(const BtEvent *event);
/* The event's text, lent; NULL if its kind has none. */
const char *bt_event_text(const BtEvent *event);
/* The event's second text (a notification's body), lent; NULL if none. */
const char *bt_event_detail(const BtEvent *event);
/* The event's yes/no. */
bool bt_event_flag(const BtEvent *event);
/* The event's number. */
int64_t bt_event_number(const BtEvent *event);
/* The event's point, in the window's points. */
double bt_event_x(const BtEvent *event);
double bt_event_y(const BtEvent *event);

/* ---- Configuration ----------------------------------------------------- */

typedef struct BtPaneConfig BtPaneConfig;

/* What the parse calls answer. */
#define BT_PARSE_CLEAN 0        /* every key accepted */
#define BT_PARSE_WITH_NOTES 1   /* read; some keys took their defaults */
#define BT_PARSE_FAILED (-1)    /* not TOML; nothing changed */
#define BT_PARSE_INVALID (-2)   /* NULL or non-UTF-8 argument; nothing changed */

/* A configuration for the pane `id` — the number its events carry, unique
 * for the process's life — of the application `app_name` (named where a pane
 * says whose it is: a downloaded file's quarantine record). Defaults: a new
 * shell in the user's home, bateri's default settings and theme, no shell
 * integration, no event handler. NULL if app_name is NULL or not UTF-8. */
BtPaneConfig *bt_pane_config_new(uint64_t id, const char *app_name);
/* Frees a configuration that was never opened. NULL is ignored. */
void bt_pane_config_free(BtPaneConfig *config);

/* The program that answers the shell integration's helper calls (bateri's
 * executable). NULL or empty: none. */
bool bt_pane_config_set_helper(BtPaneConfig *config, const char *path);
/* The shell integration's zsh scripts (bateri's Resources/shell/zsh). NULL or
 * empty: no integration — a plain terminal, without the dock and the command
 * blocks. */
bool bt_pane_config_set_zsh_scripts(BtPaneConfig *config, const char *dir);
/* Where the shell starts. NULL or empty: the user's home. */
bool bt_pane_config_set_working_directory(BtPaneConfig *config, const char *dir);
/* Adds name=value to the shell's environment. The locale's and the shell
 * integration's own win on a clash; TERM and the pane's identity
 * (TERM_SESSION_ID, ...) are never overridden. false for an empty name or one
 * holding '='. */
bool bt_pane_config_add_env(BtPaneConfig *config, const char *name, const char *value);
/* A line the shell runs once it is ready. NULL clears it. */
bool bt_pane_config_set_command(BtPaneConfig *config, const char *line);
/* Where the pane's events go. NULL handler: nowhere. */
bool bt_pane_config_set_event_handler(BtPaneConfig *config, BtEventHandler handler, void *context);
/* The pane's settings from settings.toml's text; a key not given takes its
 * default. Answers a BT_PARSE_* code; `notes`, if not NULL, gets what was not
 * accepted (or why nothing was read) one per line, the caller's to free, else
 * NULL. */
int32_t bt_pane_config_set_settings_toml(BtPaneConfig *config, const char *toml, char **notes);
/* The pane's colours: a built-in theme by name ("bateri", "bateri-light",
 * ...). false for a name it does not have. */
bool bt_pane_config_set_theme_named(BtPaneConfig *config, const char *name);
/* The pane's colours from a theme file's text; a key not given comes from
 * "bateri". Answers as bt_pane_config_set_settings_toml. */
int32_t bt_pane_config_set_theme_toml(BtPaneConfig *config, const char *toml, char **notes);

/* ---- The pane ------------------------------------------------------------ */

typedef struct BtPane BtPane;

/* Opens a pane filling `parent` (an NSView *) and following its size;
 * consumes `config` whatever the outcome. The shell starts with
 * bt_pane_start once parent is in a window. NULL if an argument is NULL or
 * the GPU could not be set up (the reason goes to stderr). */
BtPane *bt_pane_open(void *parent, BtPaneConfig *config);
/* Starts the pane's shell. false if it could not (the reason goes to stderr). */
bool bt_pane_start(BtPane *pane);
/* Gives the keyboard to the pane. false if it is in no window or the window
 * refused. */
bool bt_pane_focus(BtPane *pane);
/* Hides or shows the pane: hidden, it draws nothing and lets the focus go. */
bool bt_pane_set_hidden(BtPane *pane, bool hidden);
/* The host hid or showed a view above the pane: the pane looks again. */
bool bt_pane_visibility_changed(BtPane *pane);
/* Writes `len` bytes to the shell as if typed. false before it started. */
bool bt_pane_write(BtPane *pane, const uint8_t *bytes, size_t len);
/* Pastes text as the user's paste would (bracketed if the program asked).
 * false before the shell started. */
bool bt_pane_paste(BtPane *pane, const char *text);
/* Changes the pane's colours to a built-in theme. */
bool bt_pane_set_theme_named(BtPane *pane, const char *name);
/* Changes the pane's colours to a theme file's text. Answers a BT_PARSE_*
 * code, `notes` as in bt_pane_config_set_settings_toml. */
int32_t bt_pane_set_theme_toml(BtPane *pane, const char *toml, char **notes);
/* The pane's title, the caller's to free. NULL before the shell started. */
char *bt_pane_title(BtPane *pane);
/* The shell's working directory as it last reported it, the caller's to
 * free. NULL before it reported one. */
char *bt_pane_working_directory(BtPane *pane);
/* The pane's persistent identity — the uppercase UUID its shell sees as
 * TERM_SESSION_ID — the caller's to free. */
char *bt_pane_uuid(BtPane *pane);
/* The smallest size the pane can be laid out at, in points — the columns
 * and rows a terminal needs at its font and screen — for
 * bt_world_set_minimum. false while it is in no window. */
bool bt_pane_min_size(BtPane *pane, double *width, double *height);
/* The id the pane was configured with. 0 for NULL. */
uint64_t bt_pane_id(BtPane *pane);
/* The pane's NSView *, owned by the pane. */
void *bt_pane_view(BtPane *pane);
/* Closes the pane: its shell is told to end, nothing waits for it, its view
 * leaves its parent and the handle is freed. No event of the pane arrives
 * after this call begins. Safe inside the pane's own event handler. */
void bt_pane_close(BtPane *pane);

/* ==== The layout engine ================================================= */
/*
 * The rules bateri's own windows follow for moving panes and tabs, for a
 * host that keeps its windows, tabs and splits in a model of its own.
 *
 *  - The host's model is the one truth; the engine keeps nothing between
 *    calls. Before it asks, the host paints a picture (BtWorld) and asks as
 *    many questions of it as it likes — a drag asks for a verdict on every
 *    pointer move of one picture. The picture is painted again once the
 *    host's model changed.
 *  - A plan is carried out by the host. Its steps come in two parts. A MAIN
 *    step whose kind the host does not know makes it REFUSE the whole plan:
 *    skipping one would leave its model wrong. An AFTER step whose kind it
 *    does not know is safely SKIPPED. New kinds may be added in later
 *    releases under this rule.
 *  - Identities are the host's: windows, tabs and panes are named by its
 *    numbers, with no assumption about their range or order. A tab a move
 *    makes is given the picture's next identity, then the one after it
 *    (wrapping), and the plan says how many it used.
 *  - Ownership: a function that is given a tree copies it, except
 *    bt_tree_split, which takes its two children. Anything lent — a subtree,
 *    a plan's tree, name or strip, a verdict's tree — lives as long as what
 *    lent it, and is never freed by the host.
 *  - Geometry is points, top-down: an area's origin is its top-left corner
 *    and y grows downward.
 *  - The engine touches no AppKit: its objects may be used from any thread,
 *    never from two at once.
 */

/* ---- Codes --------------------------------------------------------------- */

#define BT_AXIS_HORIZONTAL 1u   /* side by side; the second subtree on the right */
#define BT_AXIS_VERTICAL 2u     /* stacked; the second subtree below */

#define BT_SIDE_LEFT 1u
#define BT_SIDE_RIGHT 2u
#define BT_SIDE_UP 3u
#define BT_SIDE_DOWN 4u

#define BT_SPACING_LONE 1u      /* a tab of one pane */
#define BT_SPACING_SPLIT 2u     /* a tab of several */

/* Why a move comes to nothing (bt_plan_new); 0 when a plan came. */
#define BT_REFUSAL_QUIET 1      /* it names nothing there is */
#define BT_REFUSAL_BEEP 2       /* not now: a window asks, or no room — beep */
#define BT_REFUSAL_STALE 3      /* Undo Move's record is out of date: beep, drop it */

#define BT_PART_MAIN 1u         /* unknown kind here: refuse the plan */
#define BT_PART_AFTER 2u        /* unknown kind here: skip the step */

/* Step kinds, with the fields each fills (window always). Panes and tabs a
 * step takes out are held by the host until a later step of the same plan
 * puts them somewhere: nothing is closed on the way. */
#define BT_STEP_RELEASE_PANE 1u     /* tab, pane: the pane leaves the tab (never its last) */
#define BT_STEP_RELEASE_TAB 2u      /* tab: leaves the window whole; a selected one hands
                                       the screen to its right neighbour (left at the end) */
#define BT_STEP_UNPACK 3u           /* tab: released before, gives up its panes, thrown away */
#define BT_STEP_FOLD 4u             /* tab, into: leaves the strip for `into` of the same
                                       window, which is selected if it was; panes taken */
#define BT_STEP_NEW_TAB 5u          /* tab, gap: the pane taken before becomes this new
                                       tab before `gap`, not selected */
#define BT_STEP_WRAP 6u             /* tab: the pane taken before becomes this tab, held in
                                       no strip yet */
#define BT_STEP_ADOPT_TAB 7u        /* tab, slot, index: the held tab joins the strip */
#define BT_STEP_MOVE_TAB 8u         /* tab, index: takes that place; the screen stays */
#define BT_STEP_DISSOLVE 9u         /* tab: one a move made comes apart; panes taken; its
                                       place stays until a PUT_STRIP */
#define BT_STEP_RESHAPE 10u         /* tab, tree, name: the tab holds exactly the tree's
                                       panes, laid out so, named so; one gone is born again
                                       under its id (not in the strip until PUT_STRIP) */
#define BT_STEP_PUT_STRIP 11u       /* strip: the window's strip is this again, selection
                                       with it */
#define BT_STEP_FIT 12u             /* tab: if its panes no longer fit their smallest sizes
                                       on this screen, even it out */
#define BT_STEP_UNDONE 13u          /* the window was put back: tell the user */
#define BT_STEP_OPEN_WINDOW 14u     /* tab, from, at: the held tab is the one tab of new
                                       window `window`, from's size, at the point if any.
                                       A host without that notion refuses the plan. */
#define BT_STEP_ADOPT_PANES 15u     /* tab, tree: the panes taken join the tab, laid out as
                                       the tree; not selected by it */
#define BT_STEP_CLOSE_IF_EMPTIED 16u /* the window closes if left without a tab */
#define BT_STEP_RAISE 17u           /* the window comes to the front, key */
#define BT_STEP_SELECT 18u          /* tab: comes on screen */
#define BT_STEP_FOCUS 19u           /* tab, pane: the keyboard goes there (into the tab's
                                       memory if the tab is not on screen) */
#define BT_STEP_PULSE 20u           /* tab: its chip glows once — where panes went */

#define BT_SLOT_AT 1u           /* before the tab at `index`, and selected */
#define BT_SLOT_END 2u          /* at the end, the selection where it was */

#define BT_VERDICT_NOTHING 1u   /* letting go changes nothing */
#define BT_VERDICT_LANDS 2u     /* the block lands: rect, tree, made_room */
#define BT_VERDICT_SWAPS 3u     /* trades places with target: rect, fits */
#define BT_VERDICT_TOO_SMALL 4u /* no room there: rect, and the edges that would take it */
#define BT_VERDICT_NO_ROOM 5u   /* fits nowhere in this tab */

#define BT_ZONE_WINDOW_EDGE 1u  /* full-length column or row on `side` */
#define BT_ZONE_BESIDE 2u       /* beside `target`, on `side` */
#define BT_ZONE_SWAP 3u         /* trade places with `target` */
#define BT_ZONE_OWN 4u          /* over the carried pane's own place */
#define BT_ZONE_OUTSIDE 5u      /* over no pane */

/* ---- Trees --------------------------------------------------------------- */

typedef struct BtTree BtTree;

/* A tree of one pane. */
BtTree *bt_tree_leaf(uint64_t pane);
/* Two trees side by side or stacked, the first taking `ratio` (strictly
 * between 0 and 1). Takes `first` and `second` whatever the outcome. NULL for
 * an unknown axis, a ratio out of range, a NULL tree or a pane in both. */
BtTree *bt_tree_split(uint32_t axis, double ratio, BtTree *first, BtTree *second);
/* A copy, the caller's — to keep a lent tree past its lender. */
BtTree *bt_tree_copy(const BtTree *tree);
/* Frees an owned tree. Never a lent one. */
void bt_tree_free(BtTree *tree);
bool bt_tree_is_leaf(const BtTree *tree);
/* A single-pane tree's pane; 0 for a split — ask bt_tree_is_leaf first. */
uint64_t bt_tree_pane(const BtTree *tree);
/* A split's axis and first share; 0 for a single pane. */
uint32_t bt_tree_axis(const BtTree *tree);
double bt_tree_ratio(const BtTree *tree);
/* A split's subtrees, lent by `tree`; NULL for a single pane. */
const BtTree *bt_tree_first(const BtTree *tree);
const BtTree *bt_tree_second(const BtTree *tree);
/* The panes in tree order (left to right, top to bottom). */
size_t bt_tree_pane_count(const BtTree *tree);
uint64_t bt_tree_pane_at(const BtTree *tree, size_t index);
/* The tree with every ratio evened out by its panes on that axis (Equalize
 * Splits), the caller's. */
BtTree *bt_tree_equalized(const BtTree *tree);
/* The tree as versioned text, the caller's to free (bt_string_free) — for
 * keeping a layout on disk. Every later build reads it. */
#define BT_TREE_TEXT_VERSION 1u
char *bt_tree_encode(const BtTree *tree);
/* The tree in text this build or an earlier one wrote, the caller's; NULL if
 * it is not one (a newer build's text included). */
BtTree *bt_tree_decode(const char *text);

/* ---- The picture ------------------------------------------------------- */

typedef struct BtWorld BtWorld;

/* An empty picture. A tab a move makes takes `next_id`, then the next one. */
BtWorld *bt_world_new(uint64_t next_id);
void bt_world_free(BtWorld *world);
/* Adds a window with no tab yet. stays_empty: it outlives its tabs (a move
 * that takes its last one leaves it open). asking: it holds a question that
 * blocks all of it; nothing moves into or out of it. */
bool bt_world_add_window(BtWorld *world, uint64_t window, bool stays_empty, bool asking);
/* Adds a tab at the end of the window's strip: its tree (copied), the pane
 * the keyboard is in, its name (NULL: none), and whether it is on screen — a
 * window's first tab is, unless another is said to be. false if the window
 * is not there, the tab is, the focus is not in the tree, or a pane of the
 * tree is in another tab. */
bool bt_world_add_tab(BtWorld *world, uint64_t window, uint64_t tab, const BtTree *tree,
                      uint64_t focus, const char *name, bool selected);
/* The tab's area, in points, top-down, and its screen's scale (2 on Retina).
 * Moves beside a pane, verdicts and layouts need it. */
bool bt_world_set_area(BtWorld *world, uint64_t tab, double x, double y, double width,
                       double height, double scale);
/* The space the tab leaves between panes, around them and at the top edge,
 * in points, for one pane or for several (BT_SPACING_*). Not given: a
 * one-pixel divider, no margin. Set after the area. */
bool bt_world_set_spacing(BtWorld *world, uint64_t tab, uint32_t panes, double between,
                          double around, double top);
/* The pane's smallest size, in points: no move, swap or divider takes it
 * below. A terminal pane's comes from bt_pane_min_size. */
bool bt_world_set_minimum(BtWorld *world, uint64_t pane, double width, double height);
/* The tab's tree with panes a and b trading places, the caller's; NULL if
 * either is not in it or a pane would go below its smallest size. */
BtTree *bt_world_tree_swapped(const BtWorld *world, uint64_t tab, uint64_t a, uint64_t b);
/* The tab's tree with divider `divider` (bt_layout_divider's index) dragged
 * to `position` (points along its axis, the area's frame), clamped at the
 * smallest sizes; the caller's. NULL if nothing moved. */
BtTree *bt_world_tree_dragged(const BtWorld *world, uint64_t tab, size_t divider,
                              double position);
/* The tab's tree with the divider nearest `pane` on `side`'s axis moved
 * `step` points toward `side` (Resize Split), clamped; the caller's. NULL if
 * nothing moved. */
BtTree *bt_world_tree_resized(const BtWorld *world, uint64_t tab, uint64_t pane, uint32_t side,
                              double step);

/* ---- Layouts ------------------------------------------------------------- */

typedef struct BtLayout BtLayout;

/* The tab's frames — the arithmetic every verdict and divider question here
 * is answered in, so what the host draws and what the engine judges agree.
 * NULL without the tab or its area. */
BtLayout *bt_layout_new(const BtWorld *world, uint64_t tab);
void bt_layout_free(BtLayout *layout);
size_t bt_layout_pane_count(const BtLayout *layout);
/* The index-th pane in tree order: its id and frame. */
bool bt_layout_pane(const BtLayout *layout, size_t index, uint64_t *pane, double *x, double *y,
                    double *width, double *height);
size_t bt_layout_divider_count(const BtLayout *layout);
/* The index-th divider: the axis of the split it divides and its line. */
bool bt_layout_divider(const BtLayout *layout, size_t index, uint32_t *axis, double *x,
                       double *y, double *width, double *height);

/* ---- Moves ----------------------------------------------------------------- */

typedef struct BtMove BtMove;
typedef struct BtRecord BtRecord;

/* The pane joins tab `into` beside its focused pane on `side`; neighbours
 * make room down to their smallest sizes. A tab's only pane joins as its tab. */
BtMove *bt_move_pane_to_tab(uint64_t pane, uint64_t into, uint32_t side);
/* The pane joins tab `into` laid out exactly as `tree` (a LANDS verdict's). */
BtMove *bt_move_pane_to_tab_planned(uint64_t pane, uint64_t into, const BtTree *tree);
/* The tab joins tab `into` as a block with its own layout; it and its name
 * are thrown away. */
BtMove *bt_move_tab_to_tab(uint64_t tab, uint64_t into, uint32_t side);
BtMove *bt_move_tab_to_tab_planned(uint64_t tab, uint64_t into, const BtTree *tree);
/* The pane becomes a tab of its own before the tab at `gap` of the window's
 * strip (the length is the end). A tab's only pane: the tab itself moves. */
BtMove *bt_move_pane_to_new_tab(uint64_t pane, uint64_t window, size_t gap);
/* The tab, or the pane as its one tab, becomes a window of its own; has_at:
 * let go at the screen point x, y (the platform's coordinates). */
BtMove *bt_move_tab_to_new_window(uint64_t tab, bool has_at, double x, double y);
BtMove *bt_move_pane_to_new_window(uint64_t pane, bool has_at, double x, double y);
/* The tab takes place `index` in the window's strip. */
BtMove *bt_move_tab_to_strip(uint64_t tab, uint64_t window, size_t index);
/* Every other window's tabs join window `into`'s at its end. */
BtMove *bt_move_merge_all_windows(uint64_t into);
/* Undo Move: the record's picture goes back as one step (copied). */
BtMove *bt_move_undo(const BtRecord *record);
void bt_move_free(BtMove *move);

/* ---- Plans --------------------------------------------------------------- */

typedef struct BtPlan BtPlan;
typedef struct BtStrip BtStrip;

/* What the move comes to in the picture: a plan, the caller's — or NULL with
 * why in `refusal` (BT_REFUSAL_*; 0 when a plan came). Neither argument is
 * changed: one picture answers any number of moves. */
BtPlan *bt_plan_new(const BtWorld *world, const BtMove *move, int32_t *refusal);
/* Frees the plan and everything it lent. */
void bt_plan_free(BtPlan *plan);
/* How many new identities the plan gave from the picture's next one. */
uint64_t bt_plan_ids_used(const BtPlan *plan);
size_t bt_plan_step_count(const BtPlan *plan, uint32_t part);
/* The step's kind (BT_STEP_*); 0 past the end. Fields a kind does not fill
 * read as 0. */
uint32_t bt_plan_step_kind(const BtPlan *plan, uint32_t part, size_t index);
uint64_t bt_plan_step_window(const BtPlan *plan, uint32_t part, size_t index);
uint64_t bt_plan_step_tab(const BtPlan *plan, uint32_t part, size_t index);
uint64_t bt_plan_step_pane(const BtPlan *plan, uint32_t part, size_t index);
uint64_t bt_plan_step_into(const BtPlan *plan, uint32_t part, size_t index);
uint64_t bt_plan_step_from(const BtPlan *plan, uint32_t part, size_t index);
size_t bt_plan_step_gap(const BtPlan *plan, uint32_t part, size_t index);
size_t bt_plan_step_index(const BtPlan *plan, uint32_t part, size_t index);
uint32_t bt_plan_step_slot(const BtPlan *plan, uint32_t part, size_t index);
/* An OPEN_WINDOW step's screen point, if it has one. */
bool bt_plan_step_at(const BtPlan *plan, uint32_t part, size_t index, double *x, double *y);
/* A RESHAPE step's tab name, lent by the plan; NULL when none. */
const char *bt_plan_step_name(const BtPlan *plan, uint32_t part, size_t index);
/* A RESHAPE or ADOPT_PANES step's tree, lent by the plan — it lives exactly
 * as long as the plan; copy it (bt_tree_copy) to keep it. */
const BtTree *bt_plan_step_tree(const BtPlan *plan, uint32_t part, size_t index);
/* A PUT_STRIP step's strip, lent by the plan, as long as the plan lives. */
const BtStrip *bt_plan_step_strip(const BtPlan *plan, uint32_t part, size_t index);
/* The record Undo Move keeps for this plan, the caller's — taken out, a
 * second call is NULL; NULL also for a move that cannot be taken back. Keep
 * it once every main step was carried out. */
BtRecord *bt_plan_take_undo(BtPlan *plan);

/* ---- Undo Move's records ----------------------------------------------- */

/* A record of the tab as it stands, for a change the host makes inside one
 * tab (a swap, a pane carried within its tab): taken before, kept after. */
BtRecord *bt_record_of_tab(const BtWorld *world, uint64_t tab);
/* Whether the record still pictures what the world holds — whether Undo
 * Move can be offered. False: drop it. */
bool bt_record_standing(const BtWorld *world, const BtRecord *record);
void bt_record_free(BtRecord *record);

/* ---- Verdicts: may it be let go here? ---------------------------------- */

typedef struct BtVerdict BtVerdict;

/* What `carried` (a pane of the tab, or a block from elsewhere; copied)
 * shows over the tab with the pointer at x, y (the area's frame), the
 * caller's: the answer the preview draws and the drop carries out. NULL
 * without the tab or its area. */
BtVerdict *bt_verdict_new(const BtWorld *world, uint64_t tab, const BtTree *carried, double x,
                          double y);
/* Frees the verdict and the tree it lent. */
void bt_verdict_free(BtVerdict *verdict);
uint32_t bt_verdict_kind(const BtVerdict *verdict);
/* The zone the pointer asks for (BT_ZONE_*), 0 for none — the host names it
 * in its own language. */
uint32_t bt_verdict_zone(const BtVerdict *verdict);
/* The zone's side (BT_SIDE_*) and the pane it is about; 0 when none. */
uint32_t bt_verdict_side(const BtVerdict *verdict);
uint64_t bt_verdict_target(const BtVerdict *verdict);
/* The zone's English word ("Left", "Swap", ...), lent — for debugging only. */
const char *bt_verdict_label(const BtVerdict *verdict);
/* LANDS: where the block lands; SWAPS: the target's frame; TOO_SMALL: the
 * region pointed at. false for the others. */
bool bt_verdict_rect(const BtVerdict *verdict, double *x, double *y, double *width,
                     double *height);
/* LANDS: neighbours shrink, or the block took more than its share. */
bool bt_verdict_made_room(const BtVerdict *verdict);
/* SWAPS: both panes keep their smallest sizes in each other's place. */
bool bt_verdict_fits(const BtVerdict *verdict);
/* LANDS: the whole tab's tree once landed, lent by the verdict. Give it to a
 * _planned move; for a pane carried within its own tab it is the tab's new
 * tree (take a bt_record_of_tab first). */
const BtTree *bt_verdict_tree(const BtVerdict *verdict);
/* TOO_SMALL: the edges of the area that would take the block. */
size_t bt_verdict_edge_count(const BtVerdict *verdict);
bool bt_verdict_edge(const BtVerdict *verdict, size_t index, uint32_t *side, double *x,
                     double *y, double *width, double *height);

/* ---- Strips: a window's tabs in order, and the selected one ------------- */
/* bateri's rules for a strip, whatever it looks like (a row of chips, a
 * sidebar list). Like the engine: any thread, never two at once. */

/* An empty strip. */
BtStrip *bt_strip_new(void);
/* A copy, the caller's — to keep a lent strip. */
BtStrip *bt_strip_copy(const BtStrip *strip);
/* Frees an owned strip. Never a lent one. */
void bt_strip_free(BtStrip *strip);
size_t bt_strip_count(const BtStrip *strip);
/* The tab at `index`; 0 past the end. */
uint64_t bt_strip_tab_at(const BtStrip *strip, size_t index);
/* The selected tab; false for an empty strip. */
bool bt_strip_selected(const BtStrip *strip, uint64_t *tab);
bool bt_strip_index_of(const BtStrip *strip, uint64_t tab, size_t *index);
/* New Tab: right of the selected one, and selected. */
bool bt_strip_insert(BtStrip *strip, uint64_t tab);
/* At the end, the selection where it was (selected into an empty strip). */
bool bt_strip_append(BtStrip *strip, uint64_t tab);
/* At `index`, and selected — a tab let go on the strip; one here moves. */
bool bt_strip_insert_at(BtStrip *strip, uint64_t tab, size_t index);
/* A new tab before the tab at `gap`, the selection where it was. */
bool bt_strip_place_new(BtStrip *strip, uint64_t tab, size_t gap);
/* Closing the selected tab selects its right neighbour, or the left one when
 * it was the last; closing another keeps the selection. */
bool bt_strip_close(BtStrip *strip, uint64_t tab);
bool bt_strip_select(BtStrip *strip, uint64_t tab);
/* To `index` (clamped), the selection staying on its tab. */
bool bt_strip_move_to(BtStrip *strip, uint64_t tab, size_t index);
/* Show Next / Previous Tab: the neighbour, wrapping at both ends. */
bool bt_strip_adjacent(const BtStrip *strip, bool forward, uint64_t *tab);
/* Command-digit: 1-8 the nth tab, 9 the last one. */
bool bt_strip_by_shortcut(const BtStrip *strip, uint8_t digit, uint64_t *tab);

#ifdef __cplusplus
}
#endif

#endif /* BT_EMBED_H */
