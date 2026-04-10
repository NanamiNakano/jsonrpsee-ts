# jsonrpsee-ts

Generate an [rpckit](https://rpckit.dev) schema from a `jsonrpsee` RPC trait.

This crate is meant for the common case where your Rust trait is already the source of truth. Add `#[export_schema]` next to `#[rpc(...)]`, and the same trait can drive:

- `jsonrpsee` server/client code
- an rpckit-compatible TypeScript schema
- `ts-rs` export of the schema and any referenced types

## Quick start

```rust
use jsonrpsee::core::SubscriptionResult;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::ErrorObjectOwned;
use jsonrpsee_ts::export_schema;
use ts_rs::TS;

#[derive(TS)]
#[ts(export)]
pub struct Hash {
    pub value: String,
}

#[derive(TS)]
#[ts(export)]
pub struct StorageKey {
    pub bytes: String,
}

#[export_schema]
#[rpc(server, client, namespace = "state")]
pub trait StateRpc<HashTy, StorageKeyTy> {
    #[method(name = "getKeys")]
    async fn storage_keys(
        &self,
        storage_key: StorageKeyTy,
        hash: Option<HashTy>,
    ) -> Result<Vec<StorageKeyTy>, ErrorObjectOwned>;

    #[subscription(name = "subscribeStorage", item = Vec<HashTy>)]
    async fn subscribe_storage(
        &self,
        keys: Option<Vec<StorageKeyTy>>,
    ) -> SubscriptionResult;
}
```

That generates `StateRpcSchema<HashTy, StorageKeyTy>`.

## Exporting

```rust
let cfg = ts_rs::Config::default();

let schema = StateRpcSchema::<Hash, StorageKey>::schema(&cfg);
let ts = StateRpcSchema::<Hash, StorageKey>::export_to_string(&cfg)?;
StateRpcSchema::<Hash, StorageKey>::export_all(&cfg)?;
```

The generated type implements `ts_rs::TS`, so it works with the usual `ts-rs` export flow.

## What gets generated

For the trait above, the schema looks like this:

```ts
export type StateRpcSchema = {
  requests: [
    {
      method: 'state_getKeys'
      params: [storage_key: StorageKey, hash?: Hash]
      return: Array<StorageKey>
    }
  ]
  subscriptions: [
    {
      method: 'state_subscribeStorage'
      params: [keys?: Array<StorageKey>]
      return: Array<Hash>
    }
  ]
}
```

When exported, the schema file also includes `import type` statements for referenced `ts-rs` types.

## Mapping rules

- `rpc(namespace = "...")` prefixes the generated RPC method name
- `#[method(name = "...")]` becomes a request entry
- `#[subscription(name = "...", item = T)]` becomes a subscription entry
- `Option<T>` parameters become optional TypeScript parameters
- `Result<T, E>` and `RpcResult<T>` use `T` as the `return` type
- `param_kind = map` renders an object parameter shape
- `#[argument(rename = "...")]` renames object keys

## Current limitations

- lifetime generics on the RPC trait are not supported
- the schema currently targets rpckit's `SchemaEntry` shape: `method`, `params`, and `return`
- subscription notification overrides like `name = "subscribe" => "override"` are parsed, but rpckit's current schema format has no separate field for them
