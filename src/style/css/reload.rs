//! Native file watching, constructed only when css_hot_reload(true) is requested.
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(120);

/// Read a watched stylesheet off-thread. Parsing stays on the UI thread because
/// Taffy's compact CSS lengths are intentionally not Send/Sync.
#[derive(Debug)]
pub(crate) struct ReloadEvent {
    pub index: usize,
    pub path: PathBuf,
    pub source: Result<String, String>,
}

pub(crate) struct ReloadManager {
    watcher: Option<RecommendedWatcher>,
    stop: Arc<AtomicBool>,
    wake: mpsc::SyncSender<()>,
    worker: Option<thread::JoinHandle<()>>,
}

pub(crate) fn absolute_file(path: &Path) -> std::io::Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()?.join(path)
    };
    // Preserve the file name, not a canonicalized inode: editors frequently save
    // by atomically replacing it. The parent directory is the watched resource.
    Ok(path.parent().unwrap().canonicalize()?.join(
        path.file_name()
            .ok_or_else(|| std::io::Error::other("CSS file name is missing"))?,
    ))
}

fn same_path(a: &Path, b: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        a.to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy())
    }
    #[cfg(not(target_os = "windows"))]
    {
        a == b
    }
}

impl ReloadManager {
    pub(crate) fn start(
        files: Vec<(usize, PathBuf)>,
        mut send: impl FnMut(ReloadEvent) + Send + 'static,
    ) -> anyhow::Result<Self> {
        let files: Arc<[(usize, PathBuf)]> = files
            .into_iter()
            .map(|(index, path)| Ok((index, absolute_file(&path)?)))
            .collect::<std::io::Result<Vec<_>>>()?
            .into();
        let pending = Arc::new(Mutex::new(HashSet::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (wake, receiver) = mpsc::sync_channel(1);
        let watch_files = files.clone();
        let watch_pending = pending.clone();
        let watch_wake = wake.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let event = match event {
                    Ok(event) => event,
                    Err(error) => {
                        eprintln!("CSS file watcher error: {error}");
                        return;
                    }
                };
                if event.kind.is_access() {
                    return;
                }
                // Normalize the same way as registered paths (notably /var ->
                // /private/var on macOS and extended path prefixes on Windows).
                // This runs only on native notifications, never on a frame/timer.
                let paths: Vec<_> = event
                    .paths
                    .iter()
                    .map(|p| absolute_file(p).unwrap_or_else(|_| p.clone()))
                    .collect();
                let mut pending = watch_pending.lock().unwrap();
                for (slot, (_, file)) in watch_files.iter().enumerate() {
                    if paths.iter().any(|path| {
                        same_path(path, file) || same_path(path, file.parent().unwrap())
                    }) {
                        pending.insert(slot);
                    }
                }
                if !pending.is_empty() {
                    let _ = watch_wake.try_send(());
                }
            })?;
        let mut parents = HashSet::new();
        for (_, file) in files.iter() {
            if parents.insert(file.parent().unwrap()) {
                watcher.watch(file.parent().unwrap(), RecursiveMode::NonRecursive)?;
            }
        }
        let worker_stop = stop.clone();
        let worker = thread::Builder::new()
            .name("voidui-css-reload".into())
            .spawn(move || {
                let mut previous: Vec<Option<Result<String, String>>> = vec![None; files.len()];
                // Recheck once after subscribing, closing the load -> watch startup race.
                pending.lock().unwrap().extend(0..files.len());
                loop {
                    if pending.lock().unwrap().is_empty() && receiver.recv().is_err() {
                        break;
                    }
                    if worker_stop.load(Ordering::Acquire) {
                        break;
                    }
                    // Only an actual event installs a debounce wait. While idle this
                    // thread blocks indefinitely, with no stat calls or polling.
                    while receiver.recv_timeout(DEFAULT_DEBOUNCE).is_ok() {
                        if worker_stop.load(Ordering::Acquire) {
                            return;
                        }
                    }
                    if worker_stop.load(Ordering::Acquire) {
                        break;
                    }
                    let mut changed: Vec<_> = pending.lock().unwrap().drain().collect();
                    changed.sort_unstable();
                    for slot in changed {
                        let (index, path) = &files[slot];
                        let source = std::fs::read_to_string(path).map_err(|e| e.to_string());
                        if previous[slot].as_ref() == Some(&source) {
                            continue;
                        }
                        previous[slot] = Some(source.clone());
                        send(ReloadEvent {
                            index: *index,
                            path: path.clone(),
                            source,
                        });
                    }
                }
            })?;
        // Force a startup re-read even if no notification arrives during setup.
        let _ = wake.try_send(());
        Ok(Self {
            watcher: Some(watcher),
            stop,
            wake,
            worker: Some(worker),
        })
    }
}
impl Drop for ReloadManager {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.watcher.take();
        let _ = self.wake.try_send(());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_atomic_save_filters_other_files_and_stops() {
        let dir = std::env::temp_dir().join(format!(
            "voidui-css-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("ui.css");
        std::fs::write(&file, "div{color:red;}").unwrap();
        let (tx, rx) = mpsc::channel();
        let manager = ReloadManager::start(vec![(7, file.clone())], move |e| {
            let _ = tx.send(e);
        })
        .unwrap();
        let initial = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(initial.source.unwrap().contains("red"));
        std::fs::write(dir.join("unrelated.txt"), "ignored").unwrap();
        assert!(rx.recv_timeout(DEFAULT_DEBOUNCE * 2).is_err());
        std::fs::write(dir.join("save.tmp"), "div{color:blue;}").unwrap();
        std::fs::rename(dir.join("save.tmp"), &file).unwrap();
        let event = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(event.index, 7);
        assert!(event.source.unwrap().contains("blue"));
        assert!(rx.recv_timeout(DEFAULT_DEBOUNCE * 3).is_err());
        drop(manager);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
