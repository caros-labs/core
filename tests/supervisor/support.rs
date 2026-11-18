use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use caros_core::supervisor::{AppSpec, Supervisor};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct TempDir(pub PathBuf);
impl TempDir {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "caros-supervisor-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn shell(id: &str, script: &str) -> AppSpec {
    let mut spec = AppSpec::new(app_id(id), id, "/bin/sh");
    spec.args = vec!["-c".into(), script.into()];
    spec.stop_timeout = Duration::from_millis(100);
    spec
}

pub fn until(supervisor: &mut Supervisor, condition: impl Fn(&Supervisor) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        supervisor.poll().unwrap();
        if condition(supervisor) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for child lifecycle"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

// Named fixtures map to opaque IDs only inside tests.
pub fn app_id(label: &str) -> caros_core::AppId {
    use std::{
        collections::BTreeMap,
        sync::{Mutex, OnceLock},
    };
    static IDS: OnceLock<Mutex<BTreeMap<String, caros_core::AppId>>> = OnceLock::new();
    let mut ids = IDS.get_or_init(Default::default).lock().unwrap();
    *ids.entry(label.into()).or_default()
}
