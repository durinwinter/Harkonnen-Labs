use typedb_driver::{Addresses, Credentials, DriverOptions, DriverTlsConfig, TypeDBDriver};

#[tokio::test]
#[ignore = "requires a live TypeDB instance: docker compose -f docker-compose.calvin.yml up -d typedb"]
async fn connects_to_local_typedb() {
    let credentials = Credentials::new("admin", "password");
    let tls_config = DriverTlsConfig::disabled();
    let options = DriverOptions::new(tls_config);
    let addresses = Addresses::try_from_address_str("localhost:1729").expect("parse address");

    let driver = TypeDBDriver::new(addresses, credentials, options)
        .await
        .expect("connect to local TypeDB");

    let dbs = driver.databases();
    let db_name = "harkonnen_connectivity_check";
    // Drop any database left behind by an earlier aborted run before creating
    // ours, so the create below is the thing actually under test rather than a
    // no-op against stale state. This also self-heals the one case the cleanup
    // at the end of this test cannot cover: a panic between here and there.
    if dbs.contains(db_name).await.expect("check database exists") {
        dbs.get(db_name)
            .await
            .expect("get database")
            .delete()
            .await
            .expect("delete stale test database");
    }
    dbs.create(db_name).await.expect("create test database");
    assert!(dbs
        .contains(db_name)
        .await
        .expect("verify database created"));

    // This test proves connectivity and nothing else, so it must not leave a
    // database behind on what is a shared local server.
    dbs.get(db_name)
        .await
        .expect("get database for cleanup")
        .delete()
        .await
        .expect("delete test database");
    assert!(
        !dbs.contains(db_name)
            .await
            .expect("verify database removed"),
        "connectivity test left {db_name} behind"
    );
}
