use std::ops::Not;

use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote, quote_spanned, ToTokens};
use venial::{Error, FnParam, Punctuated, TypeExpr};

pub struct NixBuiltinParams {
    pub decl: Vec<TokenStream>,
    pub def: Vec<TokenStream>,

    spans: Vec<Span>,
}

impl NixBuiltinParams {
    pub fn new(
        struct_name: &Ident,
        params: &Punctuated<FnParam>,
    ) -> Result<NixBuiltinParams, Error> {
        let params = params
            .items()
            .filter_map(|param| match param {
                venial::FnParam::Receiver(receiver) => {
                    Some(Err(Error::new_at_tokens(receiver, "self is not permitted")))
                }
                venial::FnParam::Typed(venial::FnTypedParam { name, ty, .. }) => {
                    ty.tokens.is_empty().not().then_some(Ok((name, ty)))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;

        let backtrace = params
            .first()
            .filter(|p| &p.0.to_string() == "backtrace")
            .filter(|p| {
                p.1.as_path()
                    .is_some_and(|p| p.into_token_stream().to_string().contains("NixBacktrace"))
            })
            .map(|p| p.0.span());

        let has_backtrace = backtrace.is_some();

        let has_backtrace_offset = has_backtrace.then_some(1).unwrap_or(0);

        let total_params = params.len() - has_backtrace_offset;

        let spans = params
            .iter()
            .skip(has_backtrace_offset)
            .map(|(ident, _)| ident.span())
            .collect();

        let (decl, def) = params
            .into_iter()
            .skip(has_backtrace_offset)
            .enumerate()
            .map(|(idx, (param, ty))| parse_param(idx, total_params, struct_name, param, ty))
            .collect::<(Vec<TokenStream>, Vec<TokenStream>)>();

        let def = if let Some(backtrace) = backtrace {
            let mut out = vec![quote_spanned! { backtrace => backtrace.clone()}];
            out.extend_from_slice(&def);
            out
        } else {
            def
        };

        Ok(NixBuiltinParams { decl, def, spans })
    }

    pub fn param_list(&self) -> Vec<Ident> {
        self.spans
            .iter()
            .skip(1)
            .enumerate()
            .map(|(idx, span)| format_ident!("__param_{idx}", span = span.clone()))
            .collect()
    }

    pub fn struct_def(&self) -> Vec<TokenStream> {
        self.spans
            .iter()
            .skip(1)
            .map(|span| quote_spanned! {span.clone() => None})
            .collect()
    }

    pub fn struct_decl(&self) -> Vec<TokenStream> {
        self.spans
            .iter()
            .skip(1)
            .map(|span| {
                quote_spanned! {span.clone() =>
                    Option<::std::rc::Rc<(
                        ::std::rc::Rc<crate::result::NixBacktrace>,
                        ::std::rc::Rc<Scope>,
                        ::rnix::ast::Expr
                    )>>
                }
            })
            .collect()
    }
}

fn parse_param(
    idx: usize,
    total_params: usize,
    struct_name: &Ident,
    param: &Ident,
    ty: &TypeExpr,
) -> (TokenStream, TokenStream) {
    let is_last = idx == total_params - 1;

    if is_last {
        let decl = quote! {};
        let def = quote_spanned! {param.span() => <#ty as crate::builtins::FromNixExpr>::from_nix_expr(backtrace, scope, argument)?};

        (decl, def)
    } else {
        let param_ident = format_ident!("__param_{idx}", span = param.span());

        let prev_params = (0..idx)
            .map(|i| format_ident!("__param_{i}", span = param.span()))
            .collect::<Vec<_>>();
        let new_param =
            quote_spanned! {ty.span() => Some(::std::rc::Rc::new((backtrace, scope, argument)))};

        let def = quote_spanned! {param.span() =>
            <#ty as crate::builtins::FromNixExpr>::from_nix_expr(#param.0.clone(), #param.1.clone(), #param.2.clone())?
        };

        let decl = quote_spanned! {ty.span() =>
            let Some(#param) = #param_ident else {
                return Ok(
                    NixValue::Builtin(::std::rc::Rc::new(Box::new(#struct_name(#(#prev_params,)* #new_param))))
                        .wrap()
                )
            };
        };

        (decl, def)
    }
}