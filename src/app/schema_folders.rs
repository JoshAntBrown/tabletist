//! What the app does for a tree that lists every schema as a folder
//! (`sidebar.schema_folders`): the folders' objects are read behind them,
//! so quick open and the filter find a table whose folder was never
//! unfolded.

use super::App;
use crate::model::ConnTabId;

/// How many schemas' objects are read without being asked for. Past it a
/// schema's are read when its folder is first unfolded.
pub(super) const READ_AHEAD: usize = 200;

impl App {
    /// Reads the objects of each schema the tree lists as a folder, where
    /// they were never read: the folders stay folded. A list that failed
    /// is left to its folder, which says so and asks again when unfolded.
    pub(super) fn read_folders(&mut self, tab: ConnTabId) {
        let show_system = self.settings.show_system_schemas;
        let Some(workspace) = self.workspace(tab) else {
            return;
        };
        if !workspace.tree.folders(workspace.driver, show_system) {
            return;
        }
        let unread: Vec<String> = workspace
            .tree
            .visible_schemas(workspace.driver, show_system)
            .into_iter()
            .take(READ_AHEAD)
            .filter(|schema| {
                workspace
                    .tree
                    .nodes
                    .get(schema)
                    .is_none_or(|node| node.objects.needs_load())
            })
            .collect();
        for schema in unread {
            self.load_objects(tab, &schema);
        }
    }
}
