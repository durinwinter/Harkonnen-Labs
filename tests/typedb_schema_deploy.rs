use futures::StreamExt;
use typedb_driver::{
    Addresses, Credentials, DriverOptions, DriverTlsConfig, TransactionType, TypeDBDriver,
};

const SCHEMA_TQL: &str = include_str!("../factory/coobie_semantic/typedb/schema.tql");

#[tokio::test]
#[ignore = "requires a live TypeDB instance: docker compose -f docker-compose.calvin.yml up -d typedb"]
async fn schema_deploys_without_error() {
    let credentials = Credentials::new("admin", "password");
    let options = DriverOptions::new(DriverTlsConfig::disabled());
    let addresses = Addresses::try_from_address_str("localhost:1729").expect("parse address");
    let driver = TypeDBDriver::new(addresses, credentials, options)
        .await
        .expect("connect to local TypeDB");

    let db_name = "harkonnen_schema_deploy_check";
    let dbs = driver.databases();
    if dbs.contains(db_name).await.expect("check database exists") {
        dbs.get(db_name)
            .await
            .expect("get database")
            .delete()
            .await
            .expect("delete stale test database");
    }
    dbs.create(db_name).await.expect("create test database");

    let tx = driver
        .transaction(db_name, TransactionType::Schema)
        .await
        .expect("open schema transaction");
    tx.query(SCHEMA_TQL).await.expect("deploy schema");
    tx.commit().await.expect("commit schema");

    // Prove the causally-connects relation and its roles resolved correctly
    // by matching its type definition back out in a read transaction.
    let read_tx = driver
        .transaction(db_name, TransactionType::Read)
        .await
        .expect("open read transaction");
    let answer = read_tx
        .query("match $r sub causally-connects; select $r;")
        .await
        .expect("query causally-connects type");
    let mut rows = answer.into_rows();
    let mut found = false;
    while let Some(row_result) = rows.next().await {
        row_result.expect("row");
        found = true;
    }
    assert!(
        found,
        "causally-connects relation type should exist after schema deploy"
    );

    // Release the read transaction before dropping the database — an open
    // transaction still holds it.
    drop(rows);
    drop(read_tx);

    // Don't leave the database behind on what is a shared local server. The
    // delete-stale-first block above covers the one case this cannot: a panic
    // between there and here.
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
        "schema deploy test left {db_name} behind"
    );
}
