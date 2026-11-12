//! Every schema as a folder in the sidebar's tree, each with its objects
//! under it, in place of the menu that shows one schema at a time.

use super::{Tree, TreeNode, TreeRow};
use tabletist_db::Driver;

/// How many schemas an engine lists before they stand as folders in the
/// tree. PostgreSQL's schemas, MySQL's databases and BigQuery's datasets
/// are folders even when there is one: the tree is the same whatever the
/// database holds. SQLite's one schema is `main`, which every file has and
/// nobody named: it has folders once a file is attached to it. A new
/// engine decides here.
pub fn folders_from(driver: Driver) -> usize {
    match driver {
        Driver::Postgres | Driver::MySql | Driver::BigQuery => 1,
        Driver::Sqlite => 2,
    }
}

impl Tree {
    /// Whether the tree lists every schema as a folder: the setting asks
    /// for it, and the engine has as many schemas as it takes
    /// ([`folders_from`]).
    pub fn folders(&self, driver: Driver, show_system: bool) -> bool {
        self.schema_folders
            && self.visible_schemas(driver, show_system).len() >= folders_from(driver)
    }

    /// The schemas whose prefix groups fold and unfold together: each
    /// unfolded folder's, or the one schema shown.
    pub fn group_schemas(&self, driver: Driver, show_system: bool) -> Vec<String> {
        if !self.folders(driver, show_system) {
            return self.shown_schema(driver, show_system).into_iter().collect();
        }
        self.visible_schemas(driver, show_system)
            .into_iter()
            .filter(|schema| self.nodes.get(schema).is_some_and(|node| node.expanded))
            .collect()
    }

    /// How many objects the schemas listed hold, of those read.
    pub fn objects_listed(&self, driver: Driver, show_system: bool) -> usize {
        self.visible_schemas(driver, show_system)
            .iter()
            .filter_map(|schema| self.nodes.get(schema)?.objects.value.as_ref())
            .map(Vec::len)
            .sum()
    }

    /// A folder for each schema, and under each unfolded one its objects.
    /// A filter keeps the schemas that hold a match among the objects
    /// loaded, unfolded.
    pub(super) fn folder_rows(&self, driver: Driver, show_system: bool, rows: &mut Vec<TreeRow>) {
        let needle = self.filter.trim().to_lowercase();
        let filtering = !needle.is_empty();
        for schema in self.visible_schemas(driver, show_system) {
            let node = self.nodes.get(&schema);
            let open = if filtering {
                node.and_then(|node| node.objects.value.as_ref())
                    .is_some_and(|objects| {
                        objects
                            .iter()
                            .any(|object| object.name.to_lowercase().contains(&needle))
                    })
            } else {
                node.is_some_and(|node| node.expanded)
            };
            if filtering && !open {
                continue;
            }
            rows.push(TreeRow {
                node: TreeNode::Schema(schema.clone()),
                depth: 0,
                label: schema.clone(),
                count: None,
                expanded: Some(open),
                loading: node.is_some_and(|node| node.objects.is_loading()),
                error: node
                    .and_then(|node| node.objects.error.as_ref())
                    .map(ToString::to_string),
                in_schema: false,
            });
            if open {
                self.schema_rows(&schema, 1, rows);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::RequestId;
    use crate::model::SchemaNode;
    use tabletist_db::{ObjectInfo, ObjectKind};

    fn table(name: &str) -> ObjectInfo {
        ObjectInfo {
            name: name.into(),
            kind: ObjectKind::Table,
            estimated_rows: None,
        }
    }

    /// `public` unfolded with a group, `archive` loaded and folded,
    /// `staging` never loaded, and `pg_catalog`.
    fn tree() -> Tree {
        let mut tree = Tree {
            schema_folders: true,
            ..Tree::default()
        };
        tree.schemas.value = Some(vec![
            "archive".into(),
            "pg_catalog".into(),
            "public".into(),
            "staging".into(),
        ]);
        let mut public = SchemaNode {
            expanded: true,
            ..SchemaNode::default()
        };
        public.objects.value = Some(vec![
            table("authors"),
            table("book_covers"),
            table("book_reviews"),
        ]);
        tree.nodes.insert("public".into(), public);
        let mut archive = SchemaNode::default();
        archive.objects.value = Some(vec![table("old_orders")]);
        tree.nodes.insert("archive".into(), archive);
        tree
    }

    /// Each row as its depth's indent and its label, a folder with its
    /// fold: `▾ public`, `  authors`.
    fn drawn(tree: &Tree, show_system: bool) -> Vec<String> {
        tree.visible_rows(Driver::Postgres, show_system)
            .iter()
            .map(|row| {
                let mark = match row.expanded {
                    Some(true) => "▾ ",
                    Some(false) => "▸ ",
                    None => "",
                };
                format!("{}{mark}{}", "  ".repeat(row.depth.into()), row.label)
            })
            .collect()
    }

    #[test]
    fn every_schema_is_a_folder_with_its_objects_under_it() {
        let mut tree = tree();
        assert_eq!(
            drawn(&tree, false),
            [
                "▸ archive",
                "▾ public",
                "  authors",
                "  ▸ book_",
                "▸ staging"
            ]
        );
        tree.nodes.get_mut("archive").unwrap().expanded = true;
        tree.nodes
            .get_mut("public")
            .unwrap()
            .open_groups
            .insert("book".into());
        assert_eq!(
            drawn(&tree, false),
            [
                "▾ archive",
                "  old_orders",
                "▾ public",
                "  authors",
                "  ▾ book_",
                "    covers",
                "    reviews",
                "▸ staging"
            ]
        );
        let rows = tree.visible_rows(Driver::Postgres, false);
        assert_eq!(rows[0].node, TreeNode::Schema("archive".into()));
        assert!(!rows[0].in_schema);
        assert!(rows[1..2].iter().all(|row| row.in_schema));
    }

    #[test]
    fn a_single_schema_is_a_folder_too_but_for_sqlites_main() {
        let mut tree = tree();
        tree.schemas.value = Some(vec!["pg_catalog".into(), "public".into()]);
        for driver in [Driver::Postgres, Driver::MySql, Driver::BigQuery] {
            assert!(tree.folders(driver, false), "{driver:?}");
        }
        assert_eq!(drawn(&tree, false), ["▾ public", "  authors", "  ▸ book_"]);
        assert_eq!(
            drawn(&tree, true),
            ["▸ pg_catalog", "▾ public", "  authors", "  ▸ book_"]
        );
        // No schemas listed, no folders.
        tree.schemas.value = Some(Vec::new());
        assert!(!tree.folders(Driver::Postgres, false));

        // SQLite's `main` alone has none; a file attached makes two.
        let mut lone = self::tree();
        lone.schemas.value = Some(vec!["public".into()]);
        assert!(!lone.folders(Driver::Sqlite, false));
        assert_eq!(
            lone.visible_rows(Driver::Sqlite, false)[0].node,
            TreeNode::Object(
                tabletist_db::ObjectRef::new("public", "authors"),
                ObjectKind::Table
            )
        );
        lone.schemas.value = Some(vec!["archive".into(), "public".into()]);
        assert!(lone.folders(Driver::Sqlite, false));
    }

    #[test]
    fn the_setting_off_shows_one_schema_as_before() {
        let mut tree = tree();
        tree.schema_folders = false;
        assert!(!tree.folders(Driver::Postgres, false));
        assert_eq!(drawn(&tree, false), ["authors", "▸ book_"]);
        assert_eq!(tree.group_schemas(Driver::Postgres, false), ["public"]);
    }

    #[test]
    fn flat_keeps_the_folders_and_drops_the_groups() {
        let mut tree = tree();
        tree.flat = true;
        assert_eq!(
            drawn(&tree, false),
            [
                "▸ archive",
                "▾ public",
                "  authors",
                "  book_covers",
                "  book_reviews",
                "▸ staging"
            ]
        );
    }

    #[test]
    fn a_filter_keeps_the_schemas_that_hold_a_match_unfolded() {
        let mut tree = tree();
        tree.filter = "ORDERS".into();
        // `archive` is folded, and shown open for its match; `public` has
        // none, and `staging` is not loaded.
        assert_eq!(drawn(&tree, false), ["▾ archive", "  old_orders"]);
        tree.filter = "o".into();
        assert_eq!(
            drawn(&tree, false),
            [
                "▾ archive",
                "  old_orders",
                "▾ public",
                "  authors",
                "  ▾ book_",
                "    covers",
                "    reviews"
            ]
        );
        // The folds are as they were once it is cleared.
        tree.filter.clear();
        assert_eq!(drawn(&tree, false)[0], "▸ archive");
    }

    #[test]
    fn a_folder_says_that_it_loads_and_what_failed() {
        let mut tree = tree();
        let mut staging = SchemaNode {
            expanded: true,
            ..SchemaNode::default()
        };
        staging.objects.start(RequestId(7));
        tree.nodes.insert("staging".into(), staging);
        let rows = tree.visible_rows(Driver::Postgres, false);
        let staging = rows.last().unwrap();
        assert_eq!(staging.node, TreeNode::Schema("staging".into()));
        assert!(staging.loading);
        assert_eq!(staging.expanded, Some(true));

        let node = tree.nodes.get_mut("staging").unwrap();
        node.objects
            .finish(RequestId(7), Err(tabletist_db::Error::Cancelled));
        let rows = tree.visible_rows(Driver::Postgres, false);
        let staging = rows.last().unwrap();
        assert!(!staging.loading);
        assert!(staging.error.is_some());
    }

    #[test]
    fn an_empty_schema_says_so_under_its_folder() {
        let mut tree = tree();
        let mut staging = SchemaNode {
            expanded: true,
            ..SchemaNode::default()
        };
        staging.objects.value = Some(Vec::new());
        tree.nodes.insert("staging".into(), staging);
        let rows = tree.visible_rows(Driver::Postgres, false);
        let empty = rows.last().unwrap();
        assert_eq!(empty.node, TreeNode::Empty("staging".into()));
        assert_eq!(empty.depth, 1);
        assert!(empty.in_schema);
    }

    #[test]
    fn groups_fold_together_in_every_unfolded_schema() {
        let mut tree = tree();
        assert_eq!(tree.group_schemas(Driver::Postgres, false), ["public"]);
        tree.nodes.get_mut("archive").unwrap().expanded = true;
        assert_eq!(
            tree.group_schemas(Driver::Postgres, false),
            ["archive", "public"]
        );
    }

    #[test]
    fn the_objects_of_the_schemas_shown_are_counted_folded_or_not() {
        let mut tree = tree();
        // `public`'s three and folded `archive`'s one.
        assert_eq!(tree.objects_listed(Driver::Postgres, false), 4);
        let mut catalog = SchemaNode::default();
        catalog.objects.value = Some(vec![table("pg_class")]);
        tree.nodes.insert("pg_catalog".into(), catalog);
        assert_eq!(tree.objects_listed(Driver::Postgres, false), 4);
        assert_eq!(tree.objects_listed(Driver::Postgres, true), 5);
    }
}
