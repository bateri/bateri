use std::collections::BTreeMap;
use std::ffi::{CStr, CString};
use std::ptr::{null, null_mut};

use super::*;

fn c(text: &str) -> CString {
    CString::new(text).expect("test text holds no NUL")
}

/// The header's `#define NAME value` lines whose name starts with `prefix`, the value as written
/// (an unsigned suffix and parentheses dropped).
fn defines(prefix: &str) -> BTreeMap<String, i64> {
    include_str!("../include/bt_embed.h")
        .lines()
        .filter_map(|line| line.strip_prefix("#define "))
        .filter_map(|rest| {
            let mut words = rest.split_whitespace();
            let name = words.next().filter(|name| name.starts_with(prefix))?;
            let value = words.next()?.trim_matches(['(', ')']).trim_end_matches('u');
            Some((name.to_owned(), value.parse().ok()?))
        })
        .collect()
}

/// The `bt_…` names called in `text` the way a declaration names them: `bt_name(`.
fn functions(text: &str, declared: impl Fn(&str) -> bool) -> Vec<String> {
    let mut names: Vec<String> = text
        .lines()
        .filter(|line| declared(line))
        .filter_map(|line| {
            let start = line.find("bt_")?;
            let rest = &line[start..];
            let end = rest.find('(')?;
            Some(rest[..end].to_owned())
        })
        .collect();
    names.sort();
    names
}

#[test]
fn the_header_declares_every_function_the_library_exports_and_no_other() {
    let header = functions(include_str!("../include/bt_embed.h"), |line| {
        !line.trim_start().starts_with('*')
            && !line.trim_start().starts_with("/*")
            && !line.starts_with("typedef")
            && line.contains("bt_")
            && line.contains('(')
    });
    let library = functions(include_str!("lib.rs"), |line| {
        line.starts_with("pub extern \"C\" fn bt_")
            || line.starts_with("pub unsafe extern \"C\" fn bt_")
    });
    assert!(!library.is_empty());
    assert_eq!(header, library);
}

#[test]
fn the_header_numbers_what_the_library_numbers() {
    let numbered = |pairs: &[(&str, i64)]| -> BTreeMap<String, i64> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), *value))
            .collect()
    };
    assert_eq!(
        defines("BT_EMBED_ABI_VERSION"),
        numbered(&[("BT_EMBED_ABI_VERSION", i64::from(ABI_VERSION))])
    );
    assert_eq!(
        defines("BT_EVENT_"),
        numbered(&[
            ("BT_EVENT_TITLE", i64::from(kind::TITLE)),
            ("BT_EVENT_SHELL_EXITED", i64::from(kind::SHELL_EXITED)),
            ("BT_EVENT_FOCUSED", i64::from(kind::FOCUSED)),
            ("BT_EVENT_ACTIVITY", i64::from(kind::ACTIVITY)),
            ("BT_EVENT_UPLOADS", i64::from(kind::UPLOADS)),
            ("BT_EVENT_NOTIFY", i64::from(kind::NOTIFY)),
            ("BT_EVENT_NOTICES", i64::from(kind::NOTICES)),
            ("BT_EVENT_FILES_DRAGGED", i64::from(kind::FILES_DRAGGED)),
            ("BT_EVENT_CARRY_PRESS", i64::from(kind::CARRY_PRESS)),
            ("BT_EVENT_QUESTIONS", i64::from(kind::QUESTIONS)),
        ])
    );
    assert_eq!(
        defines("BT_NOTICE_SOURCE_"),
        numbered(&[
            ("BT_NOTICE_SOURCE_WRITE", notice_source::WRITE),
            ("BT_NOTICE_SOURCE_SETTINGS", notice_source::SETTINGS),
            ("BT_NOTICE_SOURCE_THEME", notice_source::THEME),
            ("BT_NOTICE_SOURCE_FONT", notice_source::FONT),
        ])
    );
    assert_eq!(
        defines("BT_PARSE_"),
        numbered(&[
            ("BT_PARSE_CLEAN", i64::from(parse::CLEAN)),
            ("BT_PARSE_WITH_NOTES", i64::from(parse::WITH_NOTES)),
            ("BT_PARSE_FAILED", i64::from(parse::FAILED)),
            ("BT_PARSE_INVALID", i64::from(parse::INVALID)),
        ])
    );
}

#[test]
fn the_interface_is_version_one() {
    assert_eq!(bt_embed_abi_version(), 1);
}

#[test]
fn a_configuration_needs_an_application_name() {
    // SAFETY: NULL is an accepted argument.
    assert!(unsafe { bt_pane_config_new(1, null()) }.is_null());
}

#[test]
fn settings_text_says_what_it_did_not_accept() {
    // SAFETY: every pointer is a live string, the configuration is freed once.
    unsafe {
        let config = bt_pane_config_new(1, c("test").as_ptr());
        let mut notes = null_mut();
        let clean = c("[terminal]\nscrollback = 500\n");
        assert_eq!(
            bt_pane_config_set_settings_toml(config, clean.as_ptr(), &raw mut notes),
            parse::CLEAN
        );
        assert!(notes.is_null());
        assert_eq!((*config).settings.scrollback, 500);

        let refused = c("[terminal]\nscrollback = \"many\"\n");
        assert_eq!(
            bt_pane_config_set_settings_toml(config, refused.as_ptr(), &raw mut notes),
            parse::WITH_NOTES
        );
        assert!(!notes.is_null());
        assert!(
            CStr::from_ptr(notes)
                .to_string_lossy()
                .contains("scrollback")
        );
        bt_string_free(notes);

        let broken = c("[terminal\n");
        assert_eq!(
            bt_pane_config_set_settings_toml(config, broken.as_ptr(), null_mut()),
            parse::FAILED
        );
        assert_eq!(
            bt_pane_config_set_settings_toml(config, null(), null_mut()),
            parse::INVALID
        );
        bt_pane_config_free(config);
    }
}

#[test]
fn an_environment_name_is_not_empty_and_holds_no_equals_sign() {
    // SAFETY: every pointer is a live string, the configuration is freed once.
    unsafe {
        let config = bt_pane_config_new(1, c("test").as_ptr());
        assert!(bt_pane_config_add_env(
            config,
            c("NAME").as_ptr(),
            c("a=b").as_ptr()
        ));
        assert!(!bt_pane_config_add_env(
            config,
            c("A=B").as_ptr(),
            c("x").as_ptr()
        ));
        assert!(!bt_pane_config_add_env(
            config,
            c("").as_ptr(),
            c("x").as_ptr()
        ));
        assert!(!bt_pane_config_add_env(config, null(), c("x").as_ptr()));
        assert_eq!((*config).env, [("NAME".to_owned(), "a=b".to_owned())]);
        bt_pane_config_free(config);
    }
}

#[test]
fn a_built_in_theme_is_chosen_by_its_name() {
    // SAFETY: every pointer is a live string, the configuration is freed once.
    unsafe {
        let config = bt_pane_config_new(1, c("test").as_ptr());
        assert!(bt_pane_config_set_theme_named(
            config,
            c("bateri-light").as_ptr()
        ));
        assert_eq!((*config).theme, Theme::BATERI_LIGHT);
        assert!(!bt_pane_config_set_theme_named(
            config,
            c("no-such-theme").as_ptr()
        ));
        assert_eq!((*config).theme, Theme::BATERI_LIGHT);
        bt_pane_config_free(config);
    }
}

#[test]
fn an_event_lends_its_text_without_a_nul_inside() {
    let event = BtEvent {
        text: c_text(b"a\0b"),
        ..BtEvent::new(kind::NOTIFY, 7)
    };
    // SAFETY: the event lives for the block.
    unsafe {
        assert_eq!(bt_event_kind(&raw const event), kind::NOTIFY);
        assert_eq!(bt_event_pane(&raw const event), 7);
        assert_eq!(
            CStr::from_ptr(bt_event_text(&raw const event)).to_str(),
            Ok("ab")
        );
        assert!(bt_event_detail(&raw const event).is_null());
    }
}

#[test]
fn null_handles_answer_their_failure_values() {
    // SAFETY: NULL is an accepted argument everywhere.
    unsafe {
        assert_eq!(bt_event_kind(null()), 0);
        assert!(bt_event_text(null()).is_null());
        assert!(!bt_pane_start(null_mut()));
        assert!(bt_pane_title(null_mut()).is_null());
        assert_eq!(bt_pane_id(null_mut()), 0);
        bt_pane_close(null_mut());
        bt_pane_config_free(null_mut());
        bt_string_free(null_mut());
    }
}
