//! Editor sub-controllers extracted from the monolithic [`Editor`] struct.

pub(super) mod ai;
pub(super) mod search;
pub(super) mod workspace;

pub(in crate::editor) use ai::AiController;
pub(in crate::editor) use search::SearchController;
pub(in crate::editor) use workspace::WorkspaceController;
