use std::fs;

use indoc::indoc;
use jsonrpsee::core::SubscriptionResult;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee_ts::export_schema;
use serde::{Deserialize, Serialize};
use tempfile::tempdir;
use ts_rs::{Config, TS};

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct HashOutput {
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct StorageKeyOutput {
    pub bytes: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct QueryFilter {
    pub prefix: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
pub struct QueryResult<Hash> {
    pub hashes: Vec<Hash>,
}

#[export_schema]
#[rpc(server, client, namespace = "state")]
pub trait StateRpc<Hash, StorageKey>
where
    Hash: std::fmt::Debug,
{
    #[method(name = "getKeys")]
    async fn storage_keys(
        &self,
        storage_key: StorageKey,
        hash: Option<Hash>,
    ) -> Result<Vec<StorageKey>, ErrorObjectOwned>;

    #[method(name = "inspect", param_kind = map)]
    async fn inspect(
        &self,
        #[argument(rename = "type")] kind: u16,
        #[argument(rename = "filter")] filter: Option<QueryFilter>,
    ) -> Result<QueryResult<Hash>, ErrorObjectOwned>;

    #[method(name = "notify")]
    fn notify(&self, message: String);

    #[subscription(name = "subscribeStorage" => "override", item = Vec<Hash>)]
    async fn subscribe_storage(&self, keys: Option<Vec<StorageKey>>) -> SubscriptionResult;

    #[subscription(name = "subscribeSync" => "sync", item = QueryResult<Hash>)]
    fn subscribe_sync(&self, keys: Option<Vec<StorageKey>>);
}

#[test]
fn builds_expected_rpckit_schema() {
    let cfg = Config::default();

    let rendered = StateRpcSchema::<HashOutput, StorageKeyOutput>::schema(&cfg).render_inline();

    assert_eq!(
        rendered,
        indoc! {r#"
            {
              requests: [
                { method: 'state_getKeys'; params: [storage_key: StorageKeyOutput, hash?: HashOutput]; return: Array<StorageKeyOutput> },
                { method: 'state_inspect'; params: { type: number; filter?: QueryFilter }; return: QueryResult<HashOutput> },
                { method: 'state_notify'; params: [message: string]; return: void },
              ];
              subscriptions: [
                { method: 'state_subscribeStorage'; params: [keys?: Array<StorageKeyOutput>]; return: Array<HashOutput> },
                { method: 'state_subscribeSync'; params: [keys?: Array<StorageKeyOutput>]; return: QueryResult<HashOutput> },
              ];
            }
        "#}
        .trim()
    );
}

#[test]
fn exports_schema_and_all_ts_rs_dependencies() {
    let tmp = tempdir().unwrap();
    let cfg = Config::default().with_out_dir(tmp.path());

    let exported = StateRpcSchema::<HashOutput, StorageKeyOutput>::export_to_string(&cfg).unwrap();

    assert!(exported.contains("import type { HashOutput } from \"./HashOutput\";"));
    assert!(exported.contains("import type { QueryFilter } from \"./QueryFilter\";"));
    assert!(exported.contains("import type { QueryResult } from \"./QueryResult\";"));
    assert!(exported.contains("import type { StorageKeyOutput } from \"./StorageKeyOutput\";"));
    assert!(exported.contains("export type StateRpcSchema = {"));
    assert!(exported.contains(
        "{ method: 'state_subscribeSync'; params: [keys?: Array<StorageKeyOutput>]; return: QueryResult<HashOutput> },"
    ));

    StateRpcSchema::<HashOutput, StorageKeyOutput>::export_all(&cfg).unwrap();

    let schema_path = tmp.path().join("StateRpcSchema.ts");
    let schema_file = fs::read_to_string(&schema_path).unwrap();

    assert_eq!(schema_file, exported);

    for file in [
        "HashOutput.ts",
        "StorageKeyOutput.ts",
        "QueryFilter.ts",
        "QueryResult.ts",
        "StateRpcSchema.ts",
    ] {
        assert!(
            tmp.path().join(file).exists(),
            "expected exported file `{file}` to exist"
        );
    }
}
