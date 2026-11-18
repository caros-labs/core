use std::{
    fs, io,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};
use tokio::net::UnixListener;

/// Never deletes pre-existing paths, even stale sockets. The owner must resolve
/// stale paths explicitly so a second server cannot unlink a live endpoint.
pub(super) struct Socket {
    pub listener: UnixListener,
    path: PathBuf,
    inode: u64,
    device: u64,
}
impl Socket {
    pub fn bind(path: &Path) -> io::Result<Self> {
        let parent = path
            .parent()
            .ok_or_else(|| io::Error::other("socket requires a private parent directory"))?;
        let metadata = fs::symlink_metadata(parent)?;
        // SAFETY: geteuid takes no arguments and only reads process credentials.
        let uid = unsafe { libc::geteuid() };
        if !metadata.is_dir() || metadata.uid() != uid || metadata.mode() & 0o077 != 0 {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "socket parent must be an owned private directory (0700)",
            ));
        }
        let listener = UnixListener::bind(path)?;
        let metadata = fs::symlink_metadata(path)?;
        let socket = Self {
            listener,
            path: path.into(),
            inode: metadata.ino(),
            device: metadata.dev(),
        };
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(socket)
    }
}
impl Drop for Socket {
    fn drop(&mut self) {
        if let Ok(metadata) = fs::symlink_metadata(&self.path)
            && metadata.ino() == self.inode
            && metadata.dev() == self.device
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}
