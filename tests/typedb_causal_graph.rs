// End-to-end proof that Coobie's causal question gets a real answer from a
// real typed TypeDB graph: connect (which deploys the Coobie semantic
// schema), seed a small hand-written fixture graph, run the live
// `CausalGraphStore::query()` path, and assert the decoded hits are correct.
//
// The seed (`factory/coobie_semantic/typedb/test_seed.tql`) is test-only
// fixture data, NOT the production write-back path (that's a deliberately
// deferred future task). This test only exercises the read/query path.

use harkonnen_labs::causal_graph::{
    CausalGraphBackend, CausalGraphConfig, CausalGraphQuery, CausalGraphStatus, CausalGraphStore,
    TypeDbCausalGraphStore,
};
use typedb_driver::{
    Addresses, Credentials, DriverOptions, DriverTlsConfig, TransactionType, TypeDBDriver,
};

const SEED_TQL: &str = include_str!("../factory/coobie_semantic/typedb/test_seed.tql");
const DB_NAME: &str = "harkonnen_e2e_query_check";

#[tokio::test]
#[ignore = "requires a live TypeDB instance: docker compose -f docker-compose.calvin.yml up -d typedb"]
async fn typed_query_returns_seeded_causal_hits() {
    // Fixed database name, so make the test re-runnable: delete any stale
    // database left by a previous run before doing anything else. This
    // mirrors the pattern in tests/typedb_schema_deploy.rs. Without this, a
    // second run would insert a second copy of the seed data on top of the
    // first (test_seed.tql has no idempotency guard of its own), doubling
    // every hit count and breaking the sort-order assertions below.
    {
        let credentials = Credentials::new("admin", "password");
        let options = DriverOptions::new(DriverTlsConfig::disabled());
        let addresses = Addresses::try_from_address_str("localhost:1729").expect("parse address");
        let driver = TypeDBDriver::new(addresses, credentials, options)
            .await
            .expect("connect to local TypeDB for pre-cleanup");
        let dbs = driver.databases();
        if dbs.contains(DB_NAME).await.expect("check database exists") {
            dbs.get(DB_NAME)
                .await
                .expect("get database")
                .delete()
                .await
                .expect("delete stale test database");
        }
    }

    let config = CausalGraphConfig {
        backend: CausalGraphBackend::TypeDb3,
        enabled: true,
        url: "localhost:1729".to_string(),
        database: DB_NAME.to_string(),
        schema_path: "factory/coobie_semantic/typedb/schema.tql".to_string(),
        reasoning_mode: "function_backed".to_string(),
    };

    // connect() creates the (now-guaranteed-fresh) database and deploys the
    // Coobie semantic schema to it.
    let store = TypeDbCausalGraphStore::connect(config)
        .await
        .expect("connect and deploy schema");

    // Seed via a raw write transaction against the same database the store
    // just created. Uses typedb_driver directly since seeding is test-only,
    // not part of the store's API.
    let credentials = Credentials::new("admin", "password");
    let options = DriverOptions::new(DriverTlsConfig::disabled());
    let addresses = Addresses::try_from_address_str("localhost:1729").expect("parse address");
    let driver = TypeDBDriver::new(addresses, credentials, options)
        .await
        .expect("connect for seeding");
    let tx = driver
        .transaction(DB_NAME, TransactionType::Write)
        .await
        .expect("open write transaction");
    tx.query(SEED_TQL).await.expect("insert seed data");
    tx.commit().await.expect("commit seed data");

    let result = store
        .query(CausalGraphQuery {
            question: "what caused recent failures on this spec?".to_string(),
            run_id: Some("seed-run-1".to_string()),
            spec_id: None,
            limit: 6,
        })
        .await
        .expect("query");

    // Prove this is a real typed-graph answer, not an empty/broken result:
    // exactly the two seeded causal links come back, correctly decoded and
    // sorted by confidence descending (the seed has two causes feeding one
    // failed effect episode, at confidence 0.85 and 0.4).
    assert_eq!(result.status, CausalGraphStatus::Ready);
    assert_eq!(result.backend, CausalGraphBackend::TypeDb3);
    assert_eq!(result.database, DB_NAME);
    assert_eq!(
        result.hits.len(),
        2,
        "expected exactly 2 causal hits, got {:?}",
        result.hits
    );

    let first = &result.hits[0];
    let second = &result.hits[1];

    // Both hits share the same failure mode (both causal links feed the same
    // failed outcome), so label is identical on both.
    assert_eq!(first.label, "WrongAnswer");
    assert_eq!(second.label, "WrongAnswer");

    // Sort order: confidence descending. 0.85/"phase_sequence" before
    // 0.4/"resource_contention".
    assert_eq!(first.confidence, 0.85);
    assert_eq!(second.confidence, 0.4);
    assert!(
        first.confidence > second.confidence,
        "hits must be sorted by confidence descending, got {} then {}",
        first.confidence,
        second.confidence
    );

    // Summary/relation-kind decoded correctly for each hit. The summary text
    // comes from the failure-mode's `summary` attribute ("Test asserted the
    // wrong value" for both hits, since both causal links feed the same
    // failed outcome/failure-mode), with the query's own relation-kind
    // suffix appended per hit.
    assert!(
        first.summary.contains("Test asserted the wrong value"),
        "unexpected summary: {}",
        first.summary
    );
    assert!(
        first.summary.contains("phase_sequence"),
        "unexpected summary: {}",
        first.summary
    );
    assert!(
        second.summary.contains("Test asserted the wrong value"),
        "unexpected summary: {}",
        second.summary
    );
    assert!(
        second.summary.contains("resource_contention"),
        "unexpected summary: {}",
        second.summary
    );

    // evidence_refs correctly scoped to the queried run.
    assert_eq!(first.evidence_refs, vec!["run:seed-run-1".to_string()]);
    assert_eq!(second.evidence_refs, vec!["run:seed-run-1".to_string()]);

    let note = result.note.expect("note should be present");
    assert!(
        note.contains("2 hit"),
        "note should report 2 hits, got: {note}"
    );

    // The seed also contains two decoy causal chains that are otherwise
    // complete/valid but must be excluded by the query's scoping filters:
    // one tagged with a different run-id ("seed-run-other"), one within
    // "seed-run-1" but with a non-"failed" outcome status. Both decoys carry
    // confidence > 0.85 (higher than the real top hit) specifically so that
    // if either scoping filter ever stopped being applied, the decoy would
    // sort to position 0 and fail the exact-value assertions above outright.
    // This assertion additionally names the cause directly if a decoy ever
    // does leak through.
    assert!(
        !result.hits.iter().any(|h| h.label.starts_with("Decoy")),
        "query returned decoy data — a scoping filter (run-id or status=\"failed\") is not being applied: {:?}",
        result.hits
    );

    // Clean up so the database doesn't linger between runs (belt-and-braces
    // alongside the pre-cleanup above, which is what actually guarantees
    // re-runnability).
    driver
        .databases()
        .get(DB_NAME)
        .await
        .expect("get database for cleanup")
        .delete()
        .await
        .expect("cleanup test database");
}
