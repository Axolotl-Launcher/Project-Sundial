//! The active memory edit belongs to the current desktop view and data scope.

use std::sync::{Arc, Weak};

use axlauncher_core::desktop::instance_settings::EditCancellation;
use axlauncher_ui::{AppContext, AsyncApp, Context, Task, WeakEntity};

type Save = Task<Result<(), String>>;

#[derive(Clone, Default)]
pub(super) struct MemoryScope(Arc<()>);

impl MemoryScope {
    pub(super) fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Default)]
pub(super) struct MemoryEdits {
    scope: MemoryScope,
    active: Weak<EditCancellation>,
}

impl MemoryEdits {
    pub(super) fn scope(&self) -> MemoryScope {
        self.scope.clone()
    }

    pub(super) fn begin(&mut self, cancel: &Arc<EditCancellation>) -> Weak<EditCancellation> {
        self.cancel_active();
        let active = Arc::downgrade(cancel);
        self.active = active.clone();
        active
    }

    pub(super) fn invalidate(&mut self) {
        self.cancel_active();
        self.scope = MemoryScope::default();
    }

    fn cancel_active(&mut self) {
        if let Some(active) = self.active.upgrade() {
            active.cancel();
        }
        self.active = Weak::new();
    }

    pub(super) fn accepts(&self, scope: &MemoryScope, active: &Weak<EditCancellation>) -> bool {
        self.scope.matches(scope) && self.active.ptr_eq(active)
    }
}

impl Drop for MemoryEdits {
    fn drop(&mut self) {
        self.cancel_active();
    }
}

impl super::RootView {
    pub(super) fn save_memory<T: Send + 'static>(
        &mut self,
        scope: MemoryScope,
        cancel: Arc<EditCancellation>,
        cx: &mut Context<'_, Self>,
        work: impl FnOnce(&EditCancellation) -> Result<T, String> + Send + 'static,
        apply: impl FnOnce(&mut Self, T, &mut Context<'_, Self>) -> Result<(), String> + 'static,
    ) -> Save {
        let active = self.memory_edits.begin(&cancel);
        let task = cx.background_spawn(async move { work(&cancel) });
        cx.spawn(async move |view, cx| {
            let result = task.await?;
            Self::finish_memory_save(view, &scope, &active, result, apply, cx)
        })
    }

    fn finish_save<T>(
        &mut self,
        scope: &MemoryScope,
        active: &Weak<EditCancellation>,
        result: T,
        apply: impl FnOnce(&mut Self, T, &mut Context<'_, Self>) -> Result<(), String>,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), String> {
        if !self.memory_edits.accepts(scope, active) {
            return Err("The settings location changed. Reopen the editor to continue.".into());
        }
        apply(self, result, cx)
    }

    fn finish_memory_save<T: Send + 'static>(
        view: WeakEntity<Self>,
        scope: &MemoryScope,
        active: &Weak<EditCancellation>,
        result: T,
        apply: impl FnOnce(&mut Self, T, &mut Context<'_, Self>) -> Result<(), String> + 'static,
        cx: &mut AsyncApp,
    ) -> Result<(), String> {
        view.update(cx, |view, cx| {
            view.finish_save(scope, active, result, apply, cx)
        })
        .map_err(|error| error.to_string())?
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/frontend/src/panel/memory_edits.rs"]
mod tests;
