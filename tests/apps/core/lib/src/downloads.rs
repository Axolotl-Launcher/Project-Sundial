use super::*;

#[test]
fn only_the_curseforge_ipc_request_carries_the_credential() {
    let value = "f".repeat(32);
    let key = ApiKey::new(&value).unwrap();
    let route = Route::Pack(PackOrigins::CurseForge);
    assert!(!serde_json::to_string(&route).unwrap().contains(&value));
    for (route, expects_key) in [
        (route, true),
        (Route::Pack(PackOrigins::Modrinth), false),
        (Route::Pack(PackOrigins::None), false),
        (Route::default(), false),
    ] {
        let worker_route = TransferRoute::new(route, Some(&key));
        let wire = serde_json::to_string(&worker_route).unwrap();
        assert!(wire.contains(&value) == expects_key);
        let decoded: TransferRoute = serde_json::from_str(&wire)
            .unwrap_or_else(|_| panic!("could not decode the private request"));
        assert!(decoded.key.as_ref().map(|key| &key.0) == expects_key.then_some(&key));
    }
}
