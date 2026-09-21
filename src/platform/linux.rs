use winit::{
    platform::{wayland::WindowAttributesExtWayland, x11::WindowAttributesExtX11},
    window::WindowAttributes,
};

pub(super) fn window_attributes(attributes: WindowAttributes, app_id: &str) -> WindowAttributes {
    // Populate both backends without choosing one globally: Winit selects the
    // available display server. Wayland app_id and X11 WM_CLASS identify the app.
    let attributes = WindowAttributesExtWayland::with_name(attributes, app_id, app_id);
    WindowAttributesExtX11::with_name(attributes, app_id, app_id)
}

/// Subscribe only for custom chrome following desktop preferences. A scoped
/// background task owns D-Bus I/O; a local task commits layout updates on the UI
/// thread. Both stop when the window closes, without a polling timer.
pub(crate) fn watch_button_layout(tree: &crate::core::widget_tree::WidgetTree) {
    use futures_util::StreamExt;
    let scope = tree.task_scope();
    let host = tree.window_host.clone();
    let (send, mut receive) = futures_channel::mpsc::unbounded();
    let background = scope.spawn_background(async move {
        use ashpd::desktop::settings::Settings;
        const NAMESPACE: &str = "org.gnome.desktop.wm.preferences";
        const KEY: &str = "button-layout";
        let settings =
            tokio::time::timeout(std::time::Duration::from_secs(3), Settings::new()).await??;
        // Subscribe before reading so a change racing with startup cannot be lost.
        let mut changes = settings
            .receive_setting_changed_with_args::<String>(NAMESPACE, KEY)
            .await?;
        let publish = |value: String| {
            if let Ok(layout) = value.parse::<crate::WindowButtonLayout>() {
                let _ = send.unbounded_send(layout);
            }
        };
        if let Ok(Ok(value)) = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            settings.read::<String>(NAMESPACE, KEY),
        )
        .await
        {
            publish(value);
        }
        while let Some(value) = changes.next().await {
            if let Ok(value) = value {
                publish(value);
            }
        }
        Ok::<_, anyhow::Error>(())
    });
    scope.spawn(async move {
        while let Some(layout) = receive.next().await {
            let mut state = host.state();
            state.button_layout = layout;
            host.update(state);
        }
        match background.await {
            Ok(Ok(())) => {}
            error => {
                log::debug!("desktop button layout unavailable; retaining fallback: {error:?}")
            }
        }
    });
}
