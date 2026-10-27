//! What one native window holds of the global Windows: a copy of the registry
//! as of `revision`, and which Window it shows. The copy lets rendering and
//! the sidebar read membership without a context; every edit goes through the
//! shared [`Containers`] and comes back here via [`ContainerView::mirror`].
use super::{Container, ContainerError, ContainerId, ContainerSet, Containers};
use gpui::App;
use herdr_client::SessionRef;

#[derive(Clone, Debug)]
pub(crate) struct ContainerView {
    set: ContainerSet,
    shown: ContainerId,
    revision: u64,
}

impl ContainerView {
    /// A view of the registry. An empty registry first gets a default Window
    /// listing `base`, so every native window always shows something. The
    /// shown Window is the first that lists `base`, else the first.
    pub(crate) fn open(base: &SessionRef, cx: &mut App) -> Self {
        if !cx.has_global::<Containers>() {
            return Self::local(base);
        }
        if Containers::read(cx, |set| set.containers().is_empty()) {
            Containers::update(cx, |store| {
                // An empty registry cannot refuse the default Window.
                let _ = store.mutate(|set| {
                    set.ensure_default(Some(base.clone()));
                    Ok(())
                });
            });
        }
        let (set, revision) = cx
            .try_global::<Containers>()
            .map(|store| (store.set().clone(), store.revision()))
            .unwrap_or_default();
        let shown = set
            .containers()
            .iter()
            .find(|c| c.members.contains(base))
            .or_else(|| set.containers().first())
            .map(|c| c.id)
            .unwrap_or_else(ContainerId::new);
        Self {
            set,
            shown,
            revision,
        }
    }

    /// A view over one default Window listing `base` that no other native
    /// window shares: for fixtures that never install the registry.
    pub(crate) fn local(base: &SessionRef) -> Self {
        let mut set = ContainerSet::default();
        set.ensure_default(Some(base.clone()));
        let shown = set
            .containers()
            .first()
            .map_or_else(ContainerId::new, |c| c.id);
        Self {
            set,
            shown,
            revision: 0,
        }
    }

    /// Edits this copy alone, for a window with no shared registry.
    pub(crate) fn edit_local<R>(
        &mut self,
        change: impl FnOnce(&mut ContainerSet) -> Result<R, ContainerError>,
    ) -> Result<R, ContainerError> {
        let mut next = self.set.clone();
        let result = change(&mut next)?;
        self.set = next;
        if self.set.get(self.shown).is_none() {
            self.shown = self.set.containers().first().map_or(self.shown, |c| c.id);
        }
        Ok(result)
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
    /// Whether any Window lists `session`; its connection is kept while one
    /// does, so switching Windows never redials.
    pub(crate) fn references(&self, session: &SessionRef) -> bool {
        self.set.references(session)
    }

    /// Every distinct session any Window lists.
    pub(crate) fn sessions(&self) -> impl Iterator<Item = &SessionRef> {
        self.set.sessions()
    }

    pub(crate) fn shown_id(&self) -> ContainerId {
        self.shown
    }

    pub(crate) fn containers(&self) -> &[Container] {
        self.set.containers()
    }

    pub(crate) fn set(&self) -> &ContainerSet {
        &self.set
    }

    pub(crate) fn active_index(&self) -> usize {
        self.set.index_of(self.shown).unwrap_or(0)
    }

    pub(crate) fn active(&self) -> &Container {
        let index = self.active_index();
        &self.set.containers()[index]
    }

    /// Shows Window `id` if the registry has it.
    pub(crate) fn show(&mut self, id: ContainerId) -> bool {
        if self.set.get(id).is_none() {
            return false;
        }
        self.shown = id;
        true
    }

    /// Shows the Window `step` away from the shown one, wrapping.
    pub(crate) fn step(&mut self, forward: bool) {
        let all = self.set.containers();
        let len = all.len();
        let at = self.active_index();
        let next = if forward {
            (at + 1) % len
        } else {
            (at + len - 1) % len
        };
        self.shown = all[next].id;
    }

    /// Adopts the registry as of `revision`. The shown Window stays unless it
    /// was closed, then the neighbor that took its place is shown. Returns
    /// whether the shown Window changed.
    pub(crate) fn mirror(&mut self, set: ContainerSet, revision: u64) -> bool {
        let at = self.active_index();
        self.revision = revision;
        let before = self.shown;
        if set.get(self.shown).is_none()
            && let Some(replacement) = set
                .containers()
                .get(at.min(set.containers().len().saturating_sub(1)))
        {
            self.shown = replacement.id;
        }
        self.set = set;
        self.shown != before
    }
}
