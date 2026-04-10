//! Generate an [`rpckit`](https://rpckit.dev) schema from a `jsonrpsee` RPC trait.
//!
//! Add [`export_schema`] next to `#[rpc(...)]` and the macro generates a
//! `<Trait>Schema` type that:
//!
//! - builds an in-memory schema with `schema(&Config)`
//! - implements [`ts_rs::TS`]
//! - can be exported with `export`, `export_all`, or `export_to_string`
//!
//! The generated schema reuses the original RPC trait metadata:
//! `namespace`, `method(name)`, `subscription(name, item)`, `param_kind`,
//! and `#[argument(rename = "...")]`.
//!
//! # Example
//!
//! ```ignore
//! use jsonrpsee::core::SubscriptionResult;
//! use jsonrpsee::proc_macros::rpc;
//! use jsonrpsee::types::ErrorObjectOwned;
//! use jsonrpsee_ts::export_schema;
//! use ts_rs::TS;
//!
//! #[derive(TS)]
//! #[ts(export)]
//! struct Hash {
//!     value: String,
//! }
//!
//! #[derive(TS)]
//! #[ts(export)]
//! struct StorageKey {
//!     bytes: String,
//! }
//!
//! #[export_schema]
//! #[rpc(server, client, namespace = "state")]
//! trait StateRpc<HashTy, StorageKeyTy> {
//!     #[method(name = "getKeys")]
//!     async fn storage_keys(
//!         &self,
//!         storage_key: StorageKeyTy,
//!         hash: Option<HashTy>,
//!     ) -> Result<Vec<StorageKeyTy>, ErrorObjectOwned>;
//!
//!     #[subscription(name = "subscribeStorage", item = Vec<HashTy>)]
//!     async fn subscribe_storage(
//!         &self,
//!         keys: Option<Vec<StorageKeyTy>>,
//!     ) -> SubscriptionResult;
//! }
//!
//! let cfg = ts_rs::Config::default();
//! let schema = StateRpcSchema::<Hash, StorageKey>::schema(&cfg);
//! println!("{}", schema.render_type_alias("StateRpcSchema"));
//! ```
//!
//! `Option<T>` parameters become optional TypeScript parameters. `Result<T, E>`
//! and `RpcResult<T>` use the success type as the generated `return` type.
//! Referenced `ts-rs` types are imported and exported automatically when using
//! `export_all`.
extern crate self as jsonrpsee_ts;

use std::fmt::{Display, Formatter};

/// Generate a `<Trait>Schema` type for a `jsonrpsee` RPC trait.
///
/// See the crate-level documentation for a complete example.
pub use jsonrpsee_ts_macros::export_schema;

/// Parameter encoding mode mirroring `jsonrpsee`'s `param_kind`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParamKind {
    Array,
    Map,
}

#[macro_export]
macro_rules! ts_ident {
    ($rs_ident:ty) => {
        <$rs_ident as ::ts_rs::TS>::ident(&Default::default())
    };
}

/// Return the TypeScript name for a `ts-rs` type.
pub fn type_name<T: ::ts_rs::TS>(cfg: &::ts_rs::Config) -> String {
    T::name(cfg)
}

/// Return the TypeScript `void` type used for methods without a return value.
pub fn void_type() -> String {
    "void".to_string()
}

/// A single rpckit parameter entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Param {
    ident: String,
    optional: bool,
    ts_ident: String,
}

impl Param {
    /// Create a required parameter.
    pub fn new(ident: &str, ts_ident: &str) -> Self {
        Self {
            ident: ident.to_string(),
            optional: false,
            ts_ident: ts_ident.to_string(),
        }
    }

    /// Mark this parameter as optional.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    fn render_array(&self) -> String {
        let mut rendered = self.ident.clone();
        if self.optional {
            rendered.push('?');
        }
        rendered.push_str(": ");
        rendered.push_str(&self.ts_ident);
        rendered
    }

    fn render_map(&self) -> String {
        let mut rendered = ts_property_name(&self.ident);
        if self.optional {
            rendered.push('?');
        }
        rendered.push_str(": ");
        rendered.push_str(&self.ts_ident);
        rendered
    }
}

fn ts_property_name(name: &str) -> String {
    if is_ts_identifier(name) {
        name.to_string()
    } else {
        format!("'{name}'")
    }
}

fn is_ts_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    if !(first == '_' || first == '$' || first.is_ascii_alphabetic()) {
        return false;
    }

    chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric())
}

/// A single rpckit schema entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Method {
    name: String,
    params: Vec<Param>,
    param_kind: ParamKind,
    return_ts_ident: String,
}

impl Method {
    /// Create a new schema entry with array-style params by default.
    pub fn new(name: &str, return_ts_ident: &str) -> Self {
        Self {
            name: name.to_string(),
            params: vec![],
            param_kind: ParamKind::Array,
            return_ts_ident: return_ts_ident.to_string(),
        }
    }

    /// Set the parameter encoding mode.
    pub fn with_param_kind(mut self, param_kind: ParamKind) -> Self {
        self.param_kind = param_kind;
        self
    }

    /// Append one parameter.
    pub fn param(mut self, param: Param) -> Self {
        self.params.push(param);
        self
    }

    fn render_params(&self) -> String {
        match self.param_kind {
            ParamKind::Array => {
                let params = self
                    .params
                    .iter()
                    .map(Param::render_array)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{params}]")
            }
            ParamKind::Map => {
                let params = self
                    .params
                    .iter()
                    .map(Param::render_map)
                    .collect::<Vec<_>>()
                    .join("; ");
                format!("{{ {params} }}")
            }
        }
    }
}

impl Display for Method {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{ method: '{}'; params: {}; return: {} }}",
            self.name,
            self.render_params(),
            self.return_ts_ident
        )
    }
}

/// In-memory rpckit schema builder used by the generated macro output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Schema {
    requests: Vec<Method>,
    subscriptions: Vec<Method>,
}

impl Display for Schema {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.render_inline())
    }
}

impl Schema {
    /// Create an empty schema.
    pub fn new() -> Self {
        Self {
            requests: vec![],
            subscriptions: vec![],
        }
    }

    /// Append a request entry.
    pub fn request(mut self, method: Method) -> Self {
        self.requests.push(method);
        self
    }

    /// Append a subscription entry.
    pub fn subscription(mut self, subscription: Method) -> Self {
        self.subscriptions.push(subscription);
        self
    }

    /// Merge two schemas together.
    pub fn merge(mut self, mut other: Self) -> Self {
        self.requests.append(&mut other.requests);
        self.subscriptions.append(&mut other.subscriptions);
        self
    }

    /// Render the schema body as an inline TypeScript object.
    pub fn render_inline(&self) -> String {
        let requests = self.render_entries(&self.requests, 2);
        let subscriptions = self.render_entries(&self.subscriptions, 2);

        format!("{{\n  requests: {requests};\n  subscriptions: {subscriptions};\n}}")
    }

    /// Render `type <ident> = ...`.
    pub fn render_type_alias(&self, ident: &str) -> String {
        format!("type {ident} = {};", self.render_inline())
    }

    fn render_entries(&self, entries: &[Method], indent: usize) -> String {
        if entries.is_empty() {
            return "[]".to_string();
        }

        let padding = " ".repeat(indent);
        let inner_padding = " ".repeat(indent + 2);
        let rendered_entries = entries
            .iter()
            .map(|entry| format!("{inner_padding}{entry},"))
            .collect::<Vec<_>>()
            .join("\n");

        format!("[\n{rendered_entries}\n{padding}]")
    }
}

impl Default for Schema {
    fn default() -> Self {
        Self::new()
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! __jsonrpsee_ts_return_type {
    ($cfg:ident, void) => {
        ::jsonrpsee_ts::void_type()
    };
    ($cfg:ident, ty($ty:ty)) => {
        ::jsonrpsee_ts::type_name::<$ty>($cfg)
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __jsonrpsee_ts_param {
    ($cfg:ident, $name:literal, $ty:ty, required) => {
        ::jsonrpsee_ts::Param::new($name, &::jsonrpsee_ts::type_name::<$ty>($cfg))
    };
    ($cfg:ident, $name:literal, $ty:ty, optional) => {
        ::jsonrpsee_ts::Param::new($name, &::jsonrpsee_ts::type_name::<$ty>($cfg)).optional()
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __jsonrpsee_ts_method {
    (
        cfg = $cfg:ident,
        name = $name:literal,
        param_kind = $param_kind:ident,
        params = [$(($param_name:literal, $param_ty:ty, $optional:ident)),* $(,)?],
        return = $($return:tt)+
    ) => {{
        ::jsonrpsee_ts::Method::new($name, &::jsonrpsee_ts::__jsonrpsee_ts_return_type!($cfg, $($return)+))
            .with_param_kind(::jsonrpsee_ts::ParamKind::$param_kind)
            $(.param(::jsonrpsee_ts::__jsonrpsee_ts_param!($cfg, $param_name, $param_ty, $optional)))*
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __jsonrpsee_ts_schema_impl {
    (
        schema = $schema_ident:ident,
        builder = $builder_fn:ident,
        builder_generics = [$($builder_generics:tt)*],
        struct_generics = [$($struct_generics:tt)*],
        marker = [$($marker:tt)*],
        impl_generics = [$($impl_generics:tt)*],
        type_generics = [$($type_generics:tt)*],
        where_clause = [$($where_clause:tt)*],
        used_types = [$($used_ty:ty),* $(,)?]
    ) => {
        #[doc(hidden)]
        pub struct $schema_ident $($struct_generics)* $($marker)* $($where_clause)*;

        impl $($impl_generics)* $schema_ident $($type_generics)* $($where_clause)* {
            pub fn schema(cfg: &::ts_rs::Config) -> ::jsonrpsee_ts::Schema {
                $builder_fn $($builder_generics)*(cfg)
            }

            pub fn export(cfg: &::ts_rs::Config) -> ::std::result::Result<(), ::ts_rs::ExportError>
            where
                Self: 'static,
            {
                <Self as ::ts_rs::TS>::export(cfg)
            }

            pub fn export_all(cfg: &::ts_rs::Config) -> ::std::result::Result<(), ::ts_rs::ExportError>
            where
                Self: 'static,
            {
                <Self as ::ts_rs::TS>::export_all(cfg)
            }

            pub fn export_to_string(
                cfg: &::ts_rs::Config,
            ) -> ::std::result::Result<::std::string::String, ::ts_rs::ExportError>
            where
                Self: 'static,
            {
                <Self as ::ts_rs::TS>::export_to_string(cfg)
            }
        }

        impl $($impl_generics)* ::ts_rs::TS for $schema_ident $($type_generics)* $($where_clause)* {
            type WithoutGenerics = Self;
            type OptionInnerType = Self;

            fn ident(_: &::ts_rs::Config) -> ::std::string::String {
                stringify!($schema_ident).to_owned()
            }

            fn name(cfg: &::ts_rs::Config) -> ::std::string::String {
                <Self as ::ts_rs::TS>::ident(cfg)
            }

            fn decl(cfg: &::ts_rs::Config) -> ::std::string::String {
                $builder_fn $($builder_generics)*(cfg).render_type_alias(&<Self as ::ts_rs::TS>::ident(cfg))
            }

            fn decl_concrete(cfg: &::ts_rs::Config) -> ::std::string::String {
                <Self as ::ts_rs::TS>::decl(cfg)
            }

            fn inline(cfg: &::ts_rs::Config) -> ::std::string::String {
                $builder_fn $($builder_generics)*(cfg).render_inline()
            }

            fn visit_dependencies(v: &mut impl ::ts_rs::TypeVisitor)
            where
                Self: 'static,
            {
                $(
                    v.visit::<$used_ty>();
                    <$used_ty as ::ts_rs::TS>::visit_generics(v);
                    <$used_ty as ::ts_rs::TS>::visit_dependencies(v);
                )*
            }

            fn output_path() -> ::std::option::Option<::std::path::PathBuf> {
                ::std::option::Option::Some(::std::path::PathBuf::from(format!(
                    "{}.ts",
                    stringify!($schema_ident),
                )))
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::{Method, Param, ParamKind, Schema};

    #[test]
    fn renders_array_and_map_params() {
        let schema = Schema::new()
            .request(
                Method::new("state_getKeys", "Array<string>")
                    .param(Param::new("storage_key", "string"))
                    .param(Param::new("hash", "string").optional()),
            )
            .request(
                Method::new("state_query", "number")
                    .with_param_kind(ParamKind::Map)
                    .param(Param::new("type", "number"))
                    .param(Param::new("include-proofs", "boolean").optional()),
            );

        assert_eq!(
            schema.render_inline(),
            "{\n  requests: [\n    { method: 'state_getKeys'; params: [storage_key: string, hash?: string]; return: Array<string> },\n    { method: 'state_query'; params: { type: number; 'include-proofs'?: boolean }; return: number },\n  ];\n  subscriptions: [];\n}"
        );
    }
}
