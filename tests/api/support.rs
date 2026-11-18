use caros_core::{
    AppId,
    api::CoreServer,
    supervisor::{AppSpec, Supervisor},
};
use std::{fs, io, os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};
use tokio::{sync::oneshot, task::JoinHandle, time::timeout};

pub struct Directory(pub PathBuf);
impl Directory {
    pub fn new() -> Self {
        let path = std::env::temp_dir().join(format!("caros-api-{}", AppId::new()));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub struct TestCore {
    pub path: PathBuf,
    pub id: AppId,
    _directory: Directory,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<io::Result<()>>>,
}
impl TestCore {
    pub fn start() -> Self {
        let directory = Directory::new();
        let path = directory.0.join("core.sock");
        let id = AppId::new();
        let mut app = AppSpec::new(id, "Test application", "/bin/sleep");
        app.args = vec!["60".into()];
        app.stop_timeout = Duration::from_millis(50);
        let mut supervisor = Supervisor::new();
        supervisor.register(app).unwrap();
        let server = CoreServer::bind(&path, supervisor).unwrap();
        let (stop, wait) = oneshot::channel();
        let task = tokio::spawn(server.run(async {
            let _ = wait.await;
        }));
        Self {
            path,
            id,
            _directory: directory,
            stop: Some(stop),
            task: Some(task),
        }
    }
    pub async fn finish(&mut self) {
        self.stop.take().unwrap().send(()).unwrap();
        let task = self.task.as_mut().unwrap();
        timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        self.task.take();
        assert!(!self.path.exists());
    }
}
impl Drop for TestCore {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
