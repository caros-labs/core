use super::support::Directory;
use caros_core::{
    AppId,
    api::{CoreServer, manifest},
    supervisor::Supervisor,
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};

#[tokio::test]
async fn socket_is_private_and_existing_paths_are_preserved() {
    let dir = Directory::new();
    let path = dir.0.join("core.sock");
    let server = CoreServer::bind(&path, Supervisor::new()).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(CoreServer::bind(&path, Supervisor::new()).is_err());
    assert!(path.exists());
    drop(server);
    assert!(!path.exists());
    fs::write(&path, "keep me").unwrap();
    assert!(CoreServer::bind(&path, Supervisor::new()).is_err());
    assert_eq!(fs::read_to_string(&path).unwrap(), "keep me");
    fs::remove_file(&path).unwrap();
    fs::set_permissions(&dir.0, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(CoreServer::bind(&path, Supervisor::new()).is_err());
}

#[tokio::test]
async fn symlink_parent_is_rejected_and_replaced_socket_is_not_deleted() {
    let dir = Directory::new();
    let link = dir.0.join("alias");
    symlink(&dir.0, &link).unwrap();
    assert!(CoreServer::bind(link.join("core.sock"), Supervisor::new()).is_err());
    let path = dir.0.join("core.sock");
    let server = CoreServer::bind(&path, Supervisor::new()).unwrap();
    fs::remove_file(&path).unwrap();
    fs::write(&path, "replacement").unwrap();
    drop(server);
    assert_eq!(fs::read_to_string(&path).unwrap(), "replacement");
}

#[test]
fn manifest_preserves_identity_across_renames_and_rejects_duplicates() {
    let dir = Directory::new();
    let path = dir.0.join("apps.json");
    let id = AppId::new();
    let entry =
        |name| serde_json::json!({"id":id,"name":name,"executable":"/bin/sleep","args":["1"]});
    for name in ["Original", "Renamed"] {
        fs::write(&path, serde_json::json!({"apps":[entry(name)]}).to_string()).unwrap();
        let supervisor = manifest::load(&path).unwrap();
        assert_eq!(supervisor.list()[0].id, id);
        assert_eq!(supervisor.list()[0].name, name);
    }
    for value in [
        serde_json::json!({"apps":[entry("A"),entry("B")]}),
        serde_json::json!({"apps":[{"id":"carplay","name":"CarPlay","executable":"/bin/sleep"}]}),
        serde_json::json!({"apps":[{"id":id,"name":"CarPlay","executable":"sleep"}]}),
    ] {
        fs::write(&path, value.to_string()).unwrap();
        assert!(manifest::load(&path).is_err());
    }
}

#[tokio::test]
async fn management_client_cannot_impersonate_a_registered_app_for_services() {
    let mut core = super::support::TestCore::start();
    let client = caros_sdk::ManagementClient::connect(&core.path)
        .await
        .unwrap();
    let result = client.services().acquire("receiver").await;
    assert!(
        matches!(result, Err(caros_sdk::Error::Remote(e)) if e.code == caros_sdk::ErrorCode::PermissionDenied)
    );
    core.finish().await;
}
