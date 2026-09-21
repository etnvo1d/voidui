//! Component-owned execution. Acquiring a scope does not start a task or a thread.
use crate::tasks::{HandlerOutput, TaskScope, host::MountHook};

/// Get this component instance's weak task handle. Repeated calls share one scope;
/// unlike state/on_mount, this accessor does not consume an ordered hook slot.
pub fn task_scope() -> TaskScope {
    super::state::component_task_scope()
}

/// Get the containing tree/window's scope. Tasks survive component unmounts but
/// are cancelled when the tree is dropped. A root reset does not close this scope.
pub fn window_task_scope() -> TaskScope {
    super::state::task_host().window_scope()
}

/// Get the application's scope. Tasks survive individual window closure while the
/// application runtime is alive. The default application exits at its last window.
pub fn app_task_scope() -> TaskScope {
    super::state::task_host().runtime().application_scope()
}

/// Run once after a successful mount update. Call unconditionally in a stable hook
/// order. Reexecuting this component does not repeat the handler or replace its
/// captured inputs; use event handlers for work triggered by later input changes.
pub fn on_mount<R: HandlerOutput>(callback: impl AsyncFnOnce() -> R + 'static) {
    let hook = super::state::use_hook(MountHook::default);
    if !hook.scheduled.replace(true) {
        let tasks = task_scope();
        super::state::task_host().defer(&hook, move || {
            tasks.spawn_handler(async move { callback().await });
        });
    }
}
