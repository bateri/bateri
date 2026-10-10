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
/* The id the pane was configured with. 0 for NULL. */
uint64_t bt_pane_id(BtPane *pane);
/* The pane's NSView *, owned by the pane. */
void *bt_pane_view(BtPane *pane);
/* Closes the pane: its shell is told to end, nothing waits for it, its view
 * leaves its parent and the handle is freed. No event of the pane arrives
 * after this call begins. Safe inside the pane's own event handler. */
void bt_pane_close(BtPane *pane);

#ifdef __cplusplus
}
#endif

#endif /* BT_EMBED_H */
