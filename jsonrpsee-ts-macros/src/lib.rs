use std::iter;

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2, TokenTree};
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::{
    Attribute, Error, FnArg, GenericArgument, GenericParam, Ident, ItemTrait, LitStr, Pat,
    ReturnType, Token, TraitItem, TraitItemFn, Type, TypeParamBound, parse_macro_input,
    parse_quote,
};

#[proc_macro_attribute]
pub fn export_schema(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr = proc_macro2::TokenStream::from(attr);
    if !attr.is_empty() {
        return Error::new(
            Span::call_site(),
            "#[export_schema] does not take arguments",
        )
        .to_compile_error()
        .into();
    }

    let item = parse_macro_input!(item as ItemTrait);

    match expand(item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand(item: ItemTrait) -> syn::Result<TokenStream2> {
    if item.generics.lifetimes().next().is_some() {
        return Err(Error::new_spanned(
            &item.generics,
            "#[export_schema] does not support lifetime generics",
        ));
    }

    let rpc_attr = find_attr(&item.attrs, "rpc").ok_or_else(|| {
        Error::new_spanned(
            &item.ident,
            "#[export_schema] must be placed on a trait that also has #[rpc(...)]",
        )
    })?;
    let rpc_config = RpcConfig::from_attr(rpc_attr)?;

    let schema_ident = format_ident!("{}Schema", item.ident);
    let builder_fn = format_ident!(
        "__jsonrpsee_ts_build_{}_schema",
        to_snake_case(&item.ident.to_string())
    );

    let used_entries = collect_entries(&item, &rpc_config)?;

    let item_generics = item.generics.clone();
    let bounded_generics = add_ts_bounds(item_generics.clone());
    let (impl_generics, ty_generics, where_clause) = bounded_generics.split_for_impl();
    let builder_generics = render_fn_generics(&bounded_generics);
    let builder_where = bounded_generics.where_clause.clone();
    let builder_turbofish = ty_generics.as_turbofish();
    let builder_body = render_schema_builder(&used_entries);
    let schema_generics = render_struct_generics(&item_generics);
    let schema_marker = render_struct_marker(&item_generics);
    let used_types = render_used_types(&used_entries);

    Ok(quote! {
        #item

        #[doc(hidden)]
        fn #builder_fn #builder_generics (cfg: &::ts_rs::Config) -> ::jsonrpsee_ts::Schema
        #builder_where
        {
            #builder_body
        }

        ::jsonrpsee_ts::__jsonrpsee_ts_schema_impl! {
            schema = #schema_ident,
            builder = #builder_fn,
            builder_generics = [#builder_turbofish],
            struct_generics = [#schema_generics],
            marker = [#schema_marker],
            impl_generics = [#impl_generics],
            type_generics = [#ty_generics],
            where_clause = [#where_clause],
            used_types = [#used_types]
        }
    })
}

fn render_struct_generics(generics: &syn::Generics) -> TokenStream2 {
    let params = &generics.params;
    if params.is_empty() {
        TokenStream2::new()
    } else {
        quote!(<#params>)
    }
}

fn render_struct_marker(generics: &syn::Generics) -> TokenStream2 {
    let type_params = generics
        .type_params()
        .map(|param| param.ident.clone())
        .collect::<Vec<_>>();

    match type_params.as_slice() {
        [] => TokenStream2::new(),
        [single] => quote!((::std::marker::PhantomData<#single>)),
        many => quote!((::std::marker::PhantomData<(#(#many),*)>)),
    }
}

fn render_schema_builder(entries: &[RpcSchemaEntry]) -> TokenStream2 {
    let requests = entries
        .iter()
        .filter(|entry| !entry.subscription)
        .map(RpcSchemaEntry::builder_tokens)
        .collect::<Vec<_>>();
    let subscriptions = entries
        .iter()
        .filter(|entry| entry.subscription)
        .map(RpcSchemaEntry::builder_tokens)
        .collect::<Vec<_>>();

    quote! {
        ::jsonrpsee_ts::Schema::new()
            #(.request(#requests))*
            #(.subscription(#subscriptions))*
    }
}

fn render_used_types(entries: &[RpcSchemaEntry]) -> TokenStream2 {
    let used_types = entries
        .iter()
        .flat_map(|entry| entry.used_types.iter())
        .collect::<Vec<_>>();

    quote!(#(#used_types),*)
}

fn collect_entries(item: &ItemTrait, rpc_config: &RpcConfig) -> syn::Result<Vec<RpcSchemaEntry>> {
    let mut entries = Vec::new();

    for trait_item in &item.items {
        let TraitItem::Fn(method) = trait_item else {
            return Err(Error::new_spanned(
                trait_item,
                "#[export_schema] only supports RPC traits that contain methods",
            ));
        };

        if let Some(attr) = find_attr(&method.attrs, "method") {
            entries.push(RpcSchemaEntry::from_method(method, attr, rpc_config)?);
            continue;
        }

        if let Some(attr) = find_attr(&method.attrs, "subscription") {
            entries.push(RpcSchemaEntry::from_subscription(method, attr, rpc_config)?);
            continue;
        }

        return Err(Error::new_spanned(
            method,
            "RPC trait methods must have either #[method(...)] or #[subscription(...)]",
        ));
    }

    if entries.is_empty() {
        return Err(Error::new_spanned(
            &item.ident,
            "RPC trait must contain at least one method or subscription",
        ));
    }

    Ok(entries)
}

fn add_ts_bounds(mut generics: syn::Generics) -> syn::Generics {
    for param in &mut generics.params {
        if let GenericParam::Type(type_param) = param {
            let has_ts_bound = type_param.bounds.iter().any(|bound| match bound {
                TypeParamBound::Trait(bound) => bound.path.is_ident("TS"),
                _ => false,
            });

            if !has_ts_bound {
                type_param.bounds.push(parse_quote!(::ts_rs::TS));
            }
        }
    }

    generics
}

fn render_fn_generics(generics: &syn::Generics) -> TokenStream2 {
    if generics.params.is_empty() {
        TokenStream2::new()
    } else {
        let params = &generics.params;
        quote!(<#params>)
    }
}

fn find_attr<'a>(attrs: &'a [Attribute], ident: &str) -> Option<&'a Attribute> {
    attrs.iter().find(|attr| attr.path().is_ident(ident))
}

#[derive(Clone)]
struct RpcConfig {
    namespace: Option<String>,
    namespace_separator: String,
}

impl RpcConfig {
    fn from_attr(attr: &Attribute) -> syn::Result<Self> {
        let args = parse_arguments(attr)?;
        let namespace = find_argument(&args, "namespace")?
            .map(Argument::string)
            .transpose()?;
        let namespace_separator = find_argument(&args, "namespace_separator")?
            .map(Argument::string)
            .transpose()?
            .unwrap_or_else(|| "_".to_string());

        Ok(Self {
            namespace,
            namespace_separator,
        })
    }

    fn rpc_method_name(&self, method: &str) -> String {
        if let Some(namespace) = &self.namespace {
            format!("{namespace}{}{method}", self.namespace_separator)
        } else {
            method.to_string()
        }
    }
}

struct RpcSchemaEntry {
    subscription: bool,
    name: String,
    param_kind: RpcParamKind,
    params: Vec<RpcParam>,
    return_kind: SchemaReturn,
    used_types: Vec<Type>,
}

impl RpcSchemaEntry {
    fn from_method(
        method: &TraitItemFn,
        attr: &Attribute,
        rpc_config: &RpcConfig,
    ) -> syn::Result<Self> {
        let args = parse_arguments(attr)?;
        let name = find_argument(&args, "name")?
            .ok_or_else(|| Error::new_spanned(attr, "#[method(...)] requires name = \"...\""))?
            .string()?;
        let param_kind = find_argument(&args, "param_kind")?
            .map(Argument::param_kind)
            .transpose()?
            .unwrap_or(RpcParamKind::Array);

        let params = collect_params(method)?;
        let return_ty = match &method.sig.output {
            ReturnType::Default => SchemaReturn::Void,
            ReturnType::Type(_, ty) => SchemaReturn::Type(extract_success_type(ty.as_ref())),
        };

        let mut used_types = params
            .iter()
            .map(RpcParam::effective_ty)
            .collect::<Vec<_>>();
        if let SchemaReturn::Type(ty) = &return_ty {
            used_types.push(ty.clone());
        }

        Ok(Self {
            subscription: false,
            name: rpc_config.rpc_method_name(&name),
            param_kind,
            params,
            return_kind: return_ty,
            used_types,
        })
    }

    fn from_subscription(
        method: &TraitItemFn,
        attr: &Attribute,
        rpc_config: &RpcConfig,
    ) -> syn::Result<Self> {
        let args = parse_arguments(attr)?;
        let name = find_argument(&args, "name")?
            .ok_or_else(|| {
                Error::new_spanned(attr, "#[subscription(...)] requires name = \"...\"")
            })?
            .name_mapping()?;
        let item = find_argument(&args, "item")?
            .ok_or_else(|| Error::new_spanned(attr, "#[subscription(...)] requires item = Type"))?
            .type_value()?;
        let param_kind = find_argument(&args, "param_kind")?
            .map(Argument::param_kind)
            .transpose()?
            .unwrap_or(RpcParamKind::Array);

        let params = collect_params(method)?;
        let mut used_types = params
            .iter()
            .map(RpcParam::effective_ty)
            .collect::<Vec<_>>();
        used_types.push(item.clone());

        Ok(Self {
            subscription: true,
            name: rpc_config.rpc_method_name(&name.name),
            param_kind,
            params,
            return_kind: SchemaReturn::Type(item),
            used_types,
        })
    }

    fn builder_tokens(&self) -> TokenStream2 {
        let name = LitStr::new(&self.name, Span::call_site());
        let param_kind = match self.param_kind {
            RpcParamKind::Array => quote!(Array),
            RpcParamKind::Map => quote!(Map),
        };
        let return_expr = match &self.return_kind {
            SchemaReturn::Type(ty) => quote!(ty(#ty)),
            SchemaReturn::Void => quote!(void),
        };
        let params = self
            .params
            .iter()
            .map(RpcParam::builder_tokens)
            .collect::<Vec<_>>();

        quote! {
            ::jsonrpsee_ts::__jsonrpsee_ts_method! {
                cfg = cfg,
                name = #name,
                param_kind = #param_kind,
                params = [#(#params),*],
                return = #return_expr
            }
        }
    }
}

enum SchemaReturn {
    Type(Type),
    Void,
}

#[derive(Clone, Copy)]
enum RpcParamKind {
    Array,
    Map,
}

#[derive(Clone)]
struct RpcParam {
    name: String,
    ty: Type,
    optional: bool,
}

impl RpcParam {
    fn effective_ty(&self) -> Type {
        self.ty.clone()
    }

    fn builder_tokens(&self) -> TokenStream2 {
        let name = LitStr::new(&self.name, Span::call_site());
        let ty = &self.ty;

        if self.optional {
            quote!((#name, #ty, optional))
        } else {
            quote!((#name, #ty, required))
        }
    }
}

fn collect_params(method: &TraitItemFn) -> syn::Result<Vec<RpcParam>> {
    method
        .sig
        .inputs
        .iter()
        .filter_map(|arg| match arg {
            FnArg::Receiver(_) => None,
            FnArg::Typed(arg) => Some(parse_param(arg)),
        })
        .collect()
}

fn parse_param(arg: &syn::PatType) -> syn::Result<RpcParam> {
    let Pat::Ident(ident) = &*arg.pat else {
        return Err(Error::new_spanned(
            &arg.pat,
            "RPC method parameters must be named identifiers",
        ));
    };

    let name = parse_argument_rename(&arg.attrs)?.unwrap_or_else(|| ident.ident.to_string());
    let (ty, optional) = unwrap_option_type(arg.ty.as_ref())
        .map(|inner| (inner, true))
        .unwrap_or_else(|| ((*arg.ty).clone(), false));

    Ok(RpcParam { name, ty, optional })
}

fn parse_argument_rename(attrs: &[Attribute]) -> syn::Result<Option<String>> {
    let Some(attr) = find_attr(attrs, "argument") else {
        return Ok(None);
    };

    let args = parse_arguments(attr)?;
    find_argument(&args, "rename")?
        .map(Argument::string)
        .transpose()
}

fn extract_success_type(ty: &Type) -> Type {
    let Type::Path(type_path) = ty else {
        return ty.clone();
    };

    let Some(segment) = type_path.path.segments.last() else {
        return ty.clone();
    };

    if !matches!(segment.ident.to_string().as_str(), "Result" | "RpcResult") {
        return ty.clone();
    }

    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return ty.clone();
    };

    args.args
        .iter()
        .find_map(|arg| match arg {
            GenericArgument::Type(ty) => Some(ty.clone()),
            _ => None,
        })
        .unwrap_or_else(|| ty.clone())
}

fn unwrap_option_type(ty: &Type) -> Option<Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }

    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };

    args.args.iter().find_map(|arg| match arg {
        GenericArgument::Type(ty) => Some(ty.clone()),
        _ => None,
    })
}

#[derive(Clone)]
struct Argument {
    label: Ident,
    tokens: TokenStream2,
}

impl Argument {
    fn string(&self) -> syn::Result<String> {
        self.parse_value::<LitStr>().map(|lit| lit.value())
    }

    fn type_value(&self) -> syn::Result<Type> {
        self.parse_value::<Type>()
    }

    fn name_mapping(&self) -> syn::Result<NameMapping> {
        self.parse_value::<NameMapping>()
    }

    fn param_kind(&self) -> syn::Result<RpcParamKind> {
        let ident = self.parse_value::<Ident>()?;
        match ident.to_string().as_str() {
            "array" => Ok(RpcParamKind::Array),
            "map" => Ok(RpcParamKind::Map),
            _ => Err(Error::new_spanned(
                ident,
                "param_kind must be either `array` or `map`",
            )),
        }
    }

    fn parse_value<T: Parse>(&self) -> syn::Result<T> {
        fn parser<T: Parse>(stream: ParseStream) -> syn::Result<T> {
            stream.parse::<Token![=]>()?;
            stream.parse::<T>()
        }

        parser.parse2(self.tokens.clone())
    }
}

fn find_argument<'a>(args: &'a [Argument], label: &str) -> syn::Result<Option<&'a Argument>> {
    let mut matches = args.iter().filter(|arg| arg.label == label);
    let first = matches.next();
    if matches.next().is_some() {
        return Err(Error::new(
            Span::call_site(),
            format!("duplicate `{label}` argument"),
        ));
    }
    Ok(first)
}

fn parse_arguments(attr: &Attribute) -> syn::Result<Vec<Argument>> {
    attr.parse_args_with(|input: ParseStream| {
        let punctuated = Punctuated::<Argument, Token![,]>::parse_terminated(input)?;
        Ok(punctuated.into_iter().collect::<Vec<_>>())
    })
}

impl Parse for Argument {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let label = input.parse()?;
        let mut scope = 0usize;
        let tokens = iter::from_fn(|| {
            if scope == 0 && input.peek(Token![,]) {
                return None;
            }

            if input.peek(Token![<]) {
                scope += 1;
            } else if input.peek(Token![>]) {
                scope = scope.saturating_sub(1);
            }

            input.parse::<TokenTree>().ok()
        })
        .collect();

        Ok(Self { label, tokens })
    }
}

struct NameMapping {
    name: String,
}

impl Parse for NameMapping {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name = input.parse::<LitStr>()?.value();
        if input.peek(Token![=>]) {
            input.parse::<Token![=>]>()?;
            let _: LitStr = input.parse()?;
        }

        Ok(Self { name })
    }
}

fn to_snake_case(input: &str) -> String {
    let mut output = String::with_capacity(input.len());

    for (idx, ch) in input.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if idx != 0 {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push(ch);
        }
    }

    output
}
