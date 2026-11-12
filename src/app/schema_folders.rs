//! What the app does for a tree that lists every schema as a folder
//! (`sidebar.schema_folders`): the folders' objects are read behind them,
//! so quick open and the filter find a table whose folder was never
//! unfolded.

use super::App;
use crate::backend::{Command, Event, RequestId, SessionId};
use crate::model::{Action, ConnTabId};
use tabletist_db::{Error, SchemaObjects};

impl App {
    /// Reads the objects of every schema the tree lists as a folder,
    /// where they were never read, in one listing
    /// (`Command::ListAllObjects`): the folders stay folded. A list that
    /// failed is left to its folder, which says so and asks again when
    /// unfolded.
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
            .filter(|schema| {
                workspace
                    .tree
                    .nodes
                    .get(schema)
                    .is_none_or(|node| node.objects.needs_load())
            })
            .collect();
        if unread.is_empty() || !self.wake_to_read(tab) {
            return;
        }
        let request = RequestId(self.next_id());
        let Some(workspace) = self.workspace_mut(tab) else {
            return;
        };
        for schema in &unread {
            workspace
                .tree
                .nodes
                .entry(schema.clone())
                .or_default()
                .objects
                .start(request);
        }
        // An open completion list says it waits for them.
        workspace.catalog_changed();
        let session = workspace.session;
        self.backend.send(Command::ListAllObjects {
            session,
            request,
            schemas: unread,
        });
    }

    /// The answer to `read_folders`: each schema's objects reach its
    /// folder as a listing of its own does (`Event::Objects`). A listing
    /// that failed as a whole fails every folder that waited for it.
    pub(super) fn objects_read(
        &mut self,
        session: SessionId,
        request: RequestId,
        result: Result<Vec<SchemaObjects>, Error>,
    ) {
        let lists = match result {
            Ok(lists) => lists,
            Err(error) => self
                .tab_for_session(session)
                .and_then(|tab| self.workspace(tab))
                .map(|workspace| {
                    workspace
                        .tree
                        .nodes
                        .iter()
                        .filter(|(_, node)| node.objects.pending == Some(request))
                        .map(|(schema, _)| (schema.clone(), Err(error.clone())))
                        .collect()
                })
                .unwrap_or_default(),
        };
        for (schema, result) in lists {
            self.apply(Action::Backend(Event::Objects {
                session,
                request,
                schema,
                result,
            }));
        }
    }

    /// `read_folders`, for the tests of the views.
    #[cfg(test)]
    pub(crate) fn read_folders_for_test(&mut self, tab: ConnTabId) {
        self.read_folders(tab);
    }
}
