//! Every schema as a folder in the sidebar's tree: its objects under it
//! once unfolded, loaded when first asked for, and the menu that picks one
//! schema where `sidebar.schema_folders` is off.

use egui::accesskit::{Role, TreeUpdate};
use tabletist_db::{Driver, ObjectInfo, ObjectKind, ObjectRef};

use super::sidebar::{GUIDE, INDENT, NAME_IN, PAST_GUIDE};
use crate::backend::{Command, Event, RequestId, SessionId};
use crate::model::{Action, ConnTabId, TreeKey, TreeNode};
use crate::testing::{Harness, bounds, last_sent, node};
use crate::theme::Look;

/// Where the macOS chevron begins in its row: the tree's inset, then 8.
const FOLD: f32 = 8.0 + 8.0;

/// A PostgreSQL tab in `look` with the schemas `archive`, `pg_catalog`,
/// `public` and `staging`: `public` unfolded with the Bookshop's grouped
/// catalog, `book_` open, and the others never loaded.
fn schemas(look: Look) -> (Harness, ConnTabId) {
    let mut harness = Harness::new();
    harness.set_look(look);
    let tab = harness.connect_fake();
    let workspace = harness.app.workspace_mut(tab).unwrap();
    workspace.driver = Driver::Postgres;
    workspace.tree.schemas.value = Some(vec![
        "archive".into(),
        "pg_catalog".into(),
        "public".into(),
        "staging".into(),
    ]);
    workspace.tree.nodes.clear();
    let node = workspace.tree.nodes.entry("public".into()).or_default();
    node.expanded = true;
    node.objects.value = Some(crate::testing::bookshop_grouped_objects());
    node.open_groups.insert("book".into());
    harness.settle();
    (harness, tab)
}

/// The folder of `schema`, as the tree's cursor holds it.
fn folder(schema: &str) -> TreeNode {
    TreeNode::Schema(schema.into())
}

/// `schema`'s table `name`, as the tree's cursor holds it.
fn table(schema: &str, name: &str) -> TreeNode {
    TreeNode::Object(ObjectRef::new(schema, name), ObjectKind::Table)
}

/// Answers the objects the tree last asked for with the tables `names`,
/// and says whose they were.
fn answer_objects(harness: &mut Harness, names: &[&str]) -> String {
    let Command::ListObjects {
        session,
        request,
        schema,
    } = last_sent(&harness.app)
    else {
        panic!("expected ListObjects");
    };
    let (session, request, schema) = (*session, *request, schema.clone());
    let objects = names
        .iter()
        .map(|name| ObjectInfo {
            name: (*name).into(),
            kind: ObjectKind::Table,
            estimated_rows: None,
        })
        .collect();
    harness.app.apply(Action::Backend(Event::Objects {
        session,
        request,
        schema: schema.clone(),
        result: Ok(objects),
    }));
    schema
}

/// Each tree item's name and the level it reports to a screen reader, in
/// the order drawn.
fn levels(tree: &TreeUpdate) -> Vec<(String, usize)> {
    let mut items: Vec<(f64, String, usize)> = tree
        .nodes
        .iter()
        .filter(|(_, node)| node.role() == Role::TreeItem)
        .map(|(_, node)| {
            (
                node.bounds().map_or(0.0, |rect| rect.y0),
                node.label().unwrap_or_default().to_owned(),
                node.level().unwrap_or_default(),
            )
        })
        .collect();
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    items
        .into_iter()
        .map(|(_, name, level)| (name, level))
        .collect()
}

fn key(harness: &mut Harness, tab: ConnTabId, key: TreeKey) {
    harness.app.apply(Action::TreeKey { tab, key });
}

fn cursor(harness: &Harness, tab: ConnTabId) -> Option<TreeNode> {
    harness.app.workspace(tab).unwrap().tree.cursor.clone()
}

fn unfolded(harness: &Harness, tab: ConnTabId, schema: &str) -> bool {
    harness
        .app
        .workspace(tab)
        .unwrap()
        .tree
        .nodes
        .get(schema)
        .is_some_and(|node| node.expanded)
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

fn settings_file(harness: &mut Harness, text: &str) {
    let text = text.to_owned();
    harness
        .app
        .apply(Action::Backend(Event::SettingsFile { text, own: false }));
}

/// Every schema is a row of the tree, the system's left out, and its
/// objects are a level under it. No menu picks a schema above them.
#[test]
fn each_schema_is_a_folder_with_its_objects_a_level_under_it() {
    let (mut harness, _) = schemas(Look::macos());
    let tree = harness.settle();
    let levels = levels(&tree);
    let names: Vec<&str> = levels.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names[..5],
        [
            "archive, schema",
            "public, schema",
            "authors, table",
            "book_, 6 tables",
            "book_awards, table"
        ]
    );
    assert_eq!(names.last(), Some(&"staging, schema"));
    assert!(!names.contains(&"pg_catalog, schema"));
    let level = |name: &str| levels.iter().find(|(found, _)| found == name).unwrap().1;
    assert_eq!(level("public, schema"), 1);
    assert_eq!(level("authors, table"), 2);
    assert_eq!(level("book_, 6 tables"), 2);
    assert_eq!(level("book_awards, table"), 3);
    assert_eq!(node(&tree, "Schema", Role::ComboBox), None);
}

/// With `sidebar.schema_folders` off the menu picks the one schema shown,
/// as before, and a change of the file is followed at once.
#[test]
fn the_setting_off_brings_back_the_menu_of_schemas() {
    let (mut harness, _) = schemas(Look::macos());
    settings_file(&mut harness, "[sidebar]\nschema_folders = false\n");
    let tree = harness.settle();
    assert!(node(&tree, "Schema", Role::ComboBox).is_some());
    assert_eq!(node(&tree, "public, schema", Role::TreeItem), None);
    let levels = levels(&tree);
    assert_eq!(levels[0], ("authors, table".to_owned(), 1));

    settings_file(&mut harness, "[sidebar]\nschema_folders = true\n");
    let tree = harness.settle();
    assert_eq!(node(&tree, "Schema", Role::ComboBox), None);
    assert!(node(&tree, "public, schema", Role::TreeItem).is_some());
}

/// SQLite's `main` alone stands without a folder, under the menu that
/// names it: every file has it. Another engine's one schema is a folder,
/// as its several are.
#[test]
fn one_schema_is_a_folder_but_for_sqlites_main() {
    let mut harness = Harness::new();
    let tab = harness.connect_fake();
    let tree = harness.settle();
    assert_eq!(node(&tree, "main, schema", Role::TreeItem), None);
    assert!(node(&tree, "Schema", Role::ComboBox).is_some());
    assert_eq!(levels(&tree)[0].1, 1);

    harness.app.workspace_mut(tab).unwrap().driver = Driver::Postgres;
    let tree = harness.settle();
    assert!(node(&tree, "main, schema", Role::TreeItem).is_some());
    assert_eq!(node(&tree, "Schema", Role::ComboBox), None);
    let levels = levels(&tree);
    assert_eq!(levels[0], ("main, schema".to_owned(), 1));
    assert_eq!(levels[1].1, 2);
}

/// A click on a folded schema asks for its objects, once, and shows them;
/// another folds it, and a third asks for nothing.
#[test]
fn a_click_unfolds_a_schema_and_loads_it_once() {
    let (mut harness, tab) = schemas(Look::macos());
    let sent = harness.app.backend.sent.len();
    harness.click("archive, schema");
    assert_eq!(harness.app.backend.sent.len(), sent + 1);
    assert_eq!(
        answer_objects(&mut harness, &["old_orders"]),
        "archive".to_owned()
    );
    assert!(harness.has("old_orders, table"));
    assert_eq!(cursor(&harness, tab), Some(folder("archive")));

    harness.click("archive, schema");
    assert!(!harness.has("old_orders, table"));
    assert!(!unfolded(&harness, tab, "archive"));
    harness.click("archive, schema");
    assert!(harness.has("old_orders, table"));
    assert_eq!(harness.app.backend.sent.len(), sent + 1);
}

/// Right unfolds the folder under the cursor and then steps into it, Left
/// climbs from an object to its folder and then folds it, and Enter folds
/// and unfolds.
#[test]
fn the_arrows_fold_unfold_and_climb_to_a_schema() {
    let (mut harness, tab) = schemas(Look::omarchy());
    harness.app.apply(Action::SetTreeCursor {
        tab,
        node: folder("archive"),
    });
    key(&mut harness, tab, TreeKey::Right);
    assert!(unfolded(&harness, tab, "archive"));
    answer_objects(&mut harness, &["old_orders"]);
    key(&mut harness, tab, TreeKey::Right);
    assert_eq!(cursor(&harness, tab), Some(table("archive", "old_orders")));
    key(&mut harness, tab, TreeKey::Left);
    assert_eq!(cursor(&harness, tab), Some(folder("archive")));
    key(&mut harness, tab, TreeKey::Left);
    assert!(!unfolded(&harness, tab, "archive"));
    assert_eq!(cursor(&harness, tab), Some(folder("archive")));
    key(&mut harness, tab, TreeKey::Enter);
    assert!(unfolded(&harness, tab, "archive"));

    // A grouped table climbs to its group, and that to its schema.
    harness.app.apply(Action::SetTreeCursor {
        tab,
        node: table("public", "book_covers"),
    });
    key(&mut harness, tab, TreeKey::Left);
    assert_eq!(
        cursor(&harness, tab),
        Some(TreeNode::Group("public".into(), "book".into()))
    );
    key(&mut harness, tab, TreeKey::Left);
    key(&mut harness, tab, TreeKey::Left);
    assert_eq!(cursor(&harness, tab), Some(folder("public")));
}

/// `za` on a table that is in no group folds its schema, and the cursor
/// goes up to it.
#[test]
fn the_fold_key_on_a_single_table_folds_its_schema() {
    let (mut harness, tab) = schemas(Look::omarchy());
    harness.app.apply(Action::SetTreeCursor {
        tab,
        node: table("public", "authors"),
    });
    key(&mut harness, tab, TreeKey::Fold);
    assert!(!unfolded(&harness, tab, "public"));
    assert_eq!(cursor(&harness, tab), Some(folder("public")));
}

/// `zM` and `zR` fold and open the groups of every unfolded schema, and
/// leave the folders as they are: opening them all would load them all.
#[test]
fn folding_every_group_reaches_each_unfolded_schema_and_no_folder() {
    let (mut harness, tab) = schemas(Look::omarchy());
    harness.click("archive, schema");
    answer_objects(&mut harness, &["old_orders", "old_refunds", "users"]);
    let open = |harness: &Harness, schema: &str| {
        let mut open: Vec<String> = harness.app.workspace(tab).unwrap().tree.nodes[schema]
            .open_groups
            .iter()
            .cloned()
            .collect();
        open.sort();
        open
    };
    let sent = harness.app.backend.sent.len();
    key(&mut harness, tab, TreeKey::OpenAll);
    assert_eq!(open(&harness, "archive"), ["old"]);
    assert_eq!(open(&harness, "public"), ["book", "publisher"]);
    assert!(!unfolded(&harness, tab, "staging"));
    assert_eq!(harness.app.backend.sent.len(), sent);

    key(&mut harness, tab, TreeKey::FoldAll);
    assert_eq!(open(&harness, "archive"), Vec::<String>::new());
    assert_eq!(open(&harness, "public"), Vec::<String>::new());
    assert!(unfolded(&harness, tab, "archive") && unfolded(&harness, tab, "public"));
}

/// A table opened from elsewhere (quick open, a recent one) unfolds its
/// schema's folder and loads it, so the tree shows where it is.
#[test]
fn opening_a_table_of_a_folded_schema_unfolds_it() {
    let (mut harness, tab) = schemas(Look::macos());
    harness.app.apply(Action::OpenObject {
        tab,
        object: ObjectRef::new("staging", "imports"),
        kind: ObjectKind::Table,
        pin: true,
    });
    assert!(unfolded(&harness, tab, "staging"));
    let asked = harness.app.backend.sent.iter().any(
        |command| matches!(command, Command::ListObjects { schema, .. } if schema == "staging"),
    );
    assert!(asked, "the folder's objects are asked for");
}

/// The filter keeps the schemas with a match among their loaded objects,
/// unfolded, and drops the rest.
#[test]
fn the_filter_keeps_the_schemas_that_hold_a_match() {
    let (mut harness, tab) = schemas(Look::omarchy());
    harness.click("archive, schema");
    answer_objects(&mut harness, &["old_orders"]);
    harness.click("archive, schema");
    harness.app.workspace_mut(tab).unwrap().tree.filter = "old".into();
    let tree = harness.settle();
    let names: Vec<String> = levels(&tree).into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, ["archive, schema", "old_orders, table"]);
}

/// macOS: a schema's name stands where a group's does without folders,
/// and everything under it one step further in.
#[test]
fn a_schemas_objects_stand_one_step_in_from_its_name() {
    let (mut harness, _) = schemas(Look::macos());
    let tree = harness.settle();
    let left = bounds(&tree, "public, schema", Role::TreeItem)
        .expect("the folder's row")
        .left();
    let drawn = |harness: &Harness, text: &str| {
        harness
            .painted_rect(text)
            .unwrap_or_else(|| panic!("{text:?} is not drawn"))
            .left()
            - left
    };
    assert!(close(drawn(&harness, "public"), FOLD + INDENT));
    assert!(close(drawn(&harness, "authors"), FOLD + 2.0 * INDENT));
    assert!(close(drawn(&harness, "book_"), FOLD + 2.0 * INDENT));
    assert!(close(
        drawn(&harness, "covers"),
        FOLD + INDENT + GUIDE + PAST_GUIDE + NAME_IN
    ));
}

/// The objects of every folder are read once the schemas are listed, the
/// folders left folded, so quick open finds a table of a schema that was
/// never unfolded.
#[test]
fn quick_open_finds_a_table_of_a_schema_never_unfolded() {
    let mut harness = Harness::new();
    let tab = harness.connect_fake();
    harness.app.workspace_mut(tab).unwrap().driver = Driver::Postgres;
    harness.app.apply(Action::RefreshTree(tab));
    let (session, request) = harness
        .app
        .backend
        .sent
        .iter()
        .rev()
        .find_map(|command| match command {
            Command::ListSchemas { session, request } => Some((*session, *request)),
            _ => None,
        })
        .expect("the schemas are asked for again");
    harness.app.apply(Action::Backend(Event::Schemas {
        session,
        request,
        result: Ok(vec!["archive".into(), "main".into(), "pg_catalog".into()]),
    }));
    // One listing asks for every folder not read yet: folded `archive`,
    // and not the system's schema, nor `main`, which its own listing reads.
    let (session, request, schemas) = all_asked(&harness).pop().expect("one listing");
    assert_eq!(schemas, ["archive"]);
    assert_eq!(all_asked(&harness).len(), 1);
    harness.app.apply(Action::Backend(Event::AllObjects {
        session,
        request,
        result: Ok(vec![(
            "archive".into(),
            Ok(vec![ObjectInfo {
                name: "old_orders".into(),
                kind: ObjectKind::Table,
                estimated_rows: None,
            }]),
        )]),
    }));
    assert!(!unfolded(&harness, tab, "archive"));
    assert!(!harness.has("old_orders, table"));

    harness.app.apply(Action::OpenQuickOpen);
    harness.app.apply(Action::QuickOpenTyped("old_ord".into()));
    let tree = harness.settle();
    let found = crate::testing::labels(&tree)
        .iter()
        .any(|label| label.contains("old_orders"));
    assert!(found, "quick open lists the folded schema's table");
}

/// The listings of every schema's objects the app sent.
fn all_asked(harness: &Harness) -> Vec<(SessionId, RequestId, Vec<String>)> {
    harness
        .app
        .backend
        .sent
        .iter()
        .filter_map(|command| match command {
            Command::ListAllObjects {
                session,
                request,
                schemas,
            } => Some((*session, *request, schemas.clone())),
            _ => None,
        })
        .collect()
}

/// A listing of every schema that fails says so on each folder that
/// waited for it, and a schema whose own listing failed on its own.
#[test]
fn a_listing_of_every_schema_that_fails_says_so_on_its_folders() {
    let (mut harness, tab) = schemas(Look::macos());
    let session = harness.app.workspace(tab).unwrap().session;
    harness.app.read_folders_for_test(tab);
    let (_, request, asked) = all_asked(&harness).pop().expect("one listing");
    assert_eq!(asked, ["archive", "staging"]);
    let loading = |harness: &Harness, schema: &str| {
        harness.app.workspace(tab).unwrap().tree.nodes[schema]
            .objects
            .is_loading()
    };
    assert!(loading(&harness, "archive") && loading(&harness, "staging"));
    harness.app.apply(Action::Backend(Event::AllObjects {
        session,
        request,
        result: Ok(vec![
            ("archive".into(), Ok(Vec::new())),
            ("staging".into(), Err(tabletist_db::Error::Cancelled)),
        ]),
    }));
    let nodes = &harness.app.workspace(tab).unwrap().tree.nodes;
    assert_eq!(nodes["archive"].objects.value.as_deref(), Some(&[][..]));
    assert!(nodes["staging"].objects.error.is_some());
    assert!(!loading(&harness, "staging"));

    // Nothing is left to read but the one that failed; asked again, and
    // failing as a whole, it fails `staging` alone.
    harness
        .app
        .workspace_mut(tab)
        .unwrap()
        .tree
        .nodes
        .remove("staging");
    harness.app.read_folders_for_test(tab);
    let (_, request, asked) = all_asked(&harness).pop().expect("a second listing");
    assert_eq!(asked, ["staging"]);
    harness.app.apply(Action::Backend(Event::AllObjects {
        session,
        request,
        result: Err(tabletist_db::Error::Cancelled),
    }));
    let nodes = &harness.app.workspace(tab).unwrap().tree.nodes;
    assert!(nodes["staging"].objects.error.is_some());
    assert!(nodes["archive"].objects.error.is_none());
    assert!(!loading(&harness, "staging"));
}

/// Where the picked schema's name stood, the header says what the folders
/// hold and how many: the objects read, or those a filter keeps.
#[test]
fn the_header_counts_the_objects_where_the_schemas_name_stood() {
    let (mut harness, tab) = schemas(Look::macos());
    harness.settle();
    let heading = harness.painted_rect("OBJECTS").expect("the heading");
    let count = harness.painted_rect("11").expect("the count");
    assert!(count.left() > heading.right(), "{heading:?} {count:?}");
    assert!(close(count.center().y, heading.center().y));
    let name = harness.painted_rect("public").expect("the folder's name");
    assert!(heading.bottom() <= name.top(), "{heading:?} {name:?}");

    harness.app.workspace_mut(tab).unwrap().tree.filter = "book".into();
    harness.settle();
    assert!(harness.painted_rect("6").is_some(), "the filter's matches");

    // One schema at a time: its name is there, and no heading.
    settings_file(&mut harness, "[sidebar]\nschema_folders = false\n");
    harness.settle();
    assert!(harness.painted_rect("OBJECTS").is_none());
}

/// Reload stands on the row's centre with the tree/flat switch, and its
/// icon ends where the search field does, 12 in.
#[test]
fn reload_is_in_line_with_the_switch_and_the_search_field() {
    let (mut harness, _) = schemas(Look::macos());
    let tree = harness.settle();
    let field = bounds(&tree, "Find any object…", Role::Button).expect("the search field");
    let reload = bounds(&tree, "Reload objects", Role::Button).expect("Reload");
    let flat = bounds(&tree, "Flat", Role::RadioButton).expect("the switch");
    assert!(
        close(reload.center().y, flat.center().y),
        "{reload:?} {flat:?}"
    );
    // The 16 pt icon in the 24 pt button: 4 from its edge.
    assert!(
        close(reload.right() - 4.0, field.right()),
        "{reload:?} {field:?}"
    );
    assert!(flat.right() < reload.left());
}
