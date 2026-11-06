//! The rules of the palette tried together: the rows a user would have, the queries a user
//! would type.

use bt_core::Settings;

use super::*;

/// The menu's commands as the shell will build them: `cmd.<selector>`, the selector's words.
fn commands() -> Vec<Row> {
    [
        ("newTab", "New Tab"),
        ("newWindow", "New Window"),
        ("splitRight", "Split Right"),
        ("splitDown", "Split Down"),
        ("showTabList", "Show All Tabs"),
        ("showNextTab", "Show Next Tab"),
        ("clearToStart", "Clear to Start"),
        ("clearScrollback", "Clear Scrollback"),
        ("makeFontBigger", "Bigger"),
        ("makeFontSmaller", "Smaller"),
        ("resetFontSize", "Actual Size"),
        ("findInScrollback", "Find…"),
        ("equalizeSplits", "Equalize Splits"),
        ("openSettings", "Settings…"),
        ("closeTab", "Close Tab"),
    ]
    .into_iter()
    .map(|(selector, title)| {
        Row::new(Kind::Command, selector, title).keywords(command_words(selector))
    })
    .collect()
}

fn themes() -> Vec<Row> {
    [
        ("bateri", "bateri"),
        ("nord", "Nord"),
        ("dracula", "Dracula"),
        ("rose-pine", "Rosé Pine"),
        ("rose-pine-moon", "Rosé Pine Moon"),
        ("rose-pine-dawn", "Rosé Pine Dawn"),
        ("catppuccin-mocha", "Catppuccin Mocha"),
        ("tokyo-night", "Tokyo Night"),
    ]
    .into_iter()
    .map(|(name, title)| {
        Row::new(Kind::Theme, name, title)
            .keywords("theme")
            .preview("appearance.theme", name)
    })
    .collect()
}

fn everything() -> Vec<Row> {
    let mut rows = commands();
    rows.extend(setting_rows(&Settings::default()));
    rows.extend(themes());
    rows
}

fn titles(hits: &[Hit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.row.title.as_str()).collect()
}

fn first(query: &str) -> Option<String> {
    search(&everything(), query, Nearness::None)
        .first()
        .map(|hit| hit.row.title.clone())
}

#[test]
fn letters_skipping_find_the_command_that_starts_each_run_at_a_word() {
    let hits = search(&everything(), "spr", Nearness::None);
    assert_eq!(first("spr").as_deref(), Some("Split Right"));
    // S, p of "Split" and R of "Right": only the title's letters are marked.
    assert_eq!(hits[0].marks, vec![0, 1, 6]);
    assert_eq!(first("sd").as_deref(), Some("Split Down"));
}

#[test]
fn scattered_letters_are_noise_not_a_hit() {
    let theme = search(&everything(), "theme", Nearness::None);
    assert!(
        !titles(&theme).contains(&"Python Environment"),
        "t-h of python and e of environment are no word of 'theme'"
    );
    let nord = search(&everything(), "nord", Nearness::None);
    assert!(!titles(&nord).contains(&"Let Programs Copy to the Clipboard"));
    assert_eq!(first("nord").as_deref(), Some("Nord"));
}

#[test]
fn side_finds_tab_position_first() {
    // Through its keyword (sidebar); Split Right is not found by "side by side" any more.
    assert_eq!(first("side").as_deref(), Some("Tab Position"));
    let hits = search(&everything(), "side", Nearness::None);
    assert!(!titles(&hits).contains(&"Split Right"));
}

#[test]
fn accents_and_case_fold_so_rose_finds_rose_pine() {
    let hits = search(&everything(), "rose", Nearness::None);
    assert_eq!(titles(&hits)[0], "Rosé Pine");
    assert_eq!(hits[0].marks, vec![0, 1, 2, 3]);
    assert_eq!(titles(&hits).len(), 3);
    assert_eq!(first("ROSÉ P").as_deref(), Some("Rosé Pine"));
}

#[test]
fn every_word_of_the_query_must_hit_and_their_order_does_not_matter() {
    assert_eq!(first("split right").as_deref(), Some("Split Right"));
    assert_eq!(first("right split").as_deref(), Some("Split Right"));
    assert!(search(&everything(), "split nord", Nearness::None).is_empty());
    assert_eq!(first("shape cursor").as_deref(), Some("Cursor Shape"));
    assert!(search(&everything(), "   ", Nearness::None).len() == everything().len());
}

#[test]
fn keywords_and_second_lines_find_what_the_title_does_not_say() {
    let zoom = search(&everything(), "zoom", Nearness::None);
    let found = titles(&zoom);
    for title in ["Bigger", "Smaller", "Actual Size", "Font Size"] {
        assert!(found.contains(&title), "{title} in {found:?}");
    }
    let cls = titles(&search(&everything(), "cls", Nearness::None)).join("|");
    assert!(cls.contains("Clear to Start") && cls.contains("Clear Scrollback"));
    // The second line: "new shells only" is Shell Integration's note.
    let shells = search(&everything(), "new shells", Nearness::None);
    assert_eq!(titles(&shells)[0], "Shell Integration");
    // A hit through a keyword marks nothing: the letters are not in the title.
    assert!(
        search(&everything(), "sidebar", Nearness::None)[0]
            .marks
            .is_empty()
    );
}

#[test]
fn a_clusters_heading_finds_its_rows() {
    let hits = search(&everything(), "themes", Nearness::None);
    assert!(
        titles(&hits).contains(&"Nord"),
        "the Themes heading is a keyword of every theme"
    );
}

#[test]
fn an_empty_query_finds_every_row_in_the_order_given() {
    let rows = everything();
    let hits = search(&rows, "", Nearness::None);
    assert_eq!(hits.len(), rows.len());
    assert_eq!(hits[0].row.id, rows[0].id);
    assert!(
        hits.iter()
            .all(|hit| hit.score == 0.0 && hit.marks.is_empty())
    );
}

#[test]
fn a_row_in_recent_is_nearer_by_twelve_points() {
    let rows = commands();
    // Without Recent "Split Down" (the shorter title) is a hair ahead.
    let plain = search(&rows, "split", Nearness::None);
    assert_eq!(titles(&plain)[0], "Split Down");
    let recent = vec!["cmd.splitRight".to_owned()];
    let near = search(&rows, "split", Nearness::Recent(&recent));
    assert_eq!(titles(&near)[0], "Split Right");
    assert!((near[0].score - plain[1].score - RECENT_BONUS).abs() < 1e-9);
}

#[test]
fn among_tabs_that_score_alike_the_one_visited_last_goes_first() {
    let tab = |id: &str| Row::new(Kind::Tab, id, "api");
    // In visiting order, the latest first: 2 was left most recently.
    let rows = [tab("2"), tab("1"), tab("3")];
    let hits = search(&rows, "api", Nearness::Visits);
    assert_eq!(
        hits.iter().map(|h| h.row.id.as_str()).collect::<Vec<_>>(),
        ["tab.2", "tab.1", "tab.3"]
    );
    assert!((hits[0].score - hits[1].score - VISIT_STEP).abs() < 1e-9);
    // The share runs out after six places: the seventh tab and on gain nothing.
    let many: Vec<Row> = (0..9).map(|n| tab(&n.to_string())).collect();
    let scores: Vec<f64> = search(&many, "api", Nearness::Visits)
        .iter()
        .map(|h| h.score)
        .collect();
    assert!((scores[0] - scores[8] - VISIT_BONUS).abs() < 1e-9);
    assert_eq!(scores[6], scores[8]);
    // A tab's place counts among all the tabs, found or not.
    let mixed = [
        Row::new(Kind::Tab, "9", "web"),
        Row::new(Kind::Tab, "8", "api"),
    ];
    let hits = search(&mixed, "api", Nearness::Visits);
    let base = score(&mixed[1], "api").unwrap().score;
    assert!((hits[0].score - base - (VISIT_BONUS - VISIT_STEP)).abs() < 1e-9);
}

#[test]
fn a_root_search_clusters_by_kind_and_ends_with_the_two_fallbacks() {
    let clusters = root(&everything(), "sp", &[]);
    let headings: Vec<&str> = clusters.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(headings.last(), Some(&"Search"));
    assert_eq!(
        headings[0], "Commands",
        "Split Right / Split Down start with it"
    );
    let tail = clusters.last().unwrap();
    assert_eq!(
        tail.hits
            .iter()
            .map(|h| h.row.id.as_str())
            .collect::<Vec<_>>(),
        ["search.find", "search.history"]
    );
    assert_eq!(tail.hits[0].row.title, "Find “sp” in This Pane");
    assert_eq!(tail.hits[1].row.title, "Search Command History for “sp”");
    // Found nothing: the fallbacks are still offered.
    let none = root(&everything(), "zzzzqq", &[]);
    assert_eq!(none.len(), 1);
    assert_eq!(none[0].title, "Search");
}

#[test]
fn a_cluster_is_cut_to_six_rows_and_the_themes_to_eight() {
    let many_commands: Vec<Row> = (0..10)
        .map(|n| Row::new(Kind::Command, &format!("c{n}"), format!("Open {n}")))
        .collect();
    let many_themes: Vec<Row> = (0..12)
        .map(|n| Row::new(Kind::Theme, &format!("t{n}"), format!("Open theme {n}")))
        .collect();
    let rows: Vec<Row> = many_commands.into_iter().chain(many_themes).collect();
    let clusters = cluster(search(&rows, "open", Nearness::None));
    let len = |heading: &str| {
        clusters
            .iter()
            .find(|c| c.title == heading)
            .map(|c| c.hits.len())
    };
    assert_eq!(len("Commands"), Some(CLUSTER_LIMIT));
    assert_eq!(len("Themes"), Some(THEME_LIMIT));
}

#[test]
fn clusters_are_ordered_by_their_best_row() {
    let rows = [
        Row::new(Kind::Command, "a", "Tabs go here").keywords(""),
        Row::new(Kind::Theme, "b", "Tabular"),
    ];
    // "Tabular" is the better title match for "tab", though it came second.
    let clusters = cluster(search(&rows, "tab", Nearness::None));
    let headings: Vec<&str> = clusters.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(headings, ["Themes", "Commands"]);
    // Rows that never sit in a cluster (a page's choices) are left out of the grouping.
    let page = [Row::new(Kind::Choice, "beam", "Beam")];
    assert!(cluster(search(&page, "beam", Nearness::None)).is_empty());
}

#[test]
fn recent_names_rows_in_its_order_and_drops_what_the_menu_no_longer_has() {
    let rows = everything();
    let recent = vec![
        "set.terminal.cursor".to_owned(),
        "cmd.gone".to_owned(),
        "cmd.splitRight".to_owned(),
    ];
    let named = recent_rows(&recent, &rows);
    assert_eq!(
        named.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["set.terminal.cursor", "cmd.splitRight"]
    );
}

#[test]
fn choosing_a_value_from_a_page_is_an_edit_the_settings_model_accepts() {
    let settings = Settings::default();
    let row = setting("terminal.cursor").unwrap().row(&settings);
    let mut palette = Palette::new();
    assert_eq!(
        palette.key(Key::Enter(Modifier::None), std::slice::from_ref(&row)),
        Effect::None
    );
    let Some(Scope::Choices(key)) = palette.scope().cloned() else {
        panic!("the row opens its page");
    };
    let page = setting(&key).unwrap().choice_rows(&settings);
    palette.arrive(&page);
    // The page lands on the value in effect (Block); one down is Underline, which previews.
    assert_eq!(palette.highlight(), 0);
    let shown = palette.key(Key::Down, &page);
    assert_eq!(
        shown,
        Effect::Preview(Some(Preview {
            key: "terminal.cursor".into(),
            token: "underline".into()
        }))
    );
    let Effect::Activate { id, .. } = palette.key(Key::Enter(Modifier::None), &page) else {
        panic!("a choice");
    };
    let token = id.strip_prefix("opt.").unwrap();
    let edit = setting(&key).unwrap().edit(token).unwrap();
    let text = Settings::with_edit("[terminal]\ncursor = \"block\" # mine\n", &edit).unwrap();
    assert!(text.contains("cursor = \"underline\" # mine"), "{text}");
}

#[test]
fn a_tab_at_the_root_is_as_near_as_it_is_on_its_page() {
    let tab = |id: &str| Row::new(Kind::Tab, id, "api");
    let rows = [tab("2"), tab("1")];
    let recent: Vec<String> = Vec::new();
    let hits = search(&rows, "api", Nearness::Recent(&recent));
    assert_eq!(
        hits.iter().map(|h| h.row.id.as_str()).collect::<Vec<_>>(),
        ["tab.2", "tab.1"]
    );
    assert!((hits[0].score - hits[1].score - VISIT_STEP).abs() < 1e-9);
}
