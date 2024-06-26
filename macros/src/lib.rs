use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{parse::Parse, visit_mut::VisitMut, Token};

struct LifetimeAdder {
    lifetime: syn::Lifetime,
}

impl VisitMut for LifetimeAdder {
    fn visit_type_reference_mut(&mut self, i: &mut syn::TypeReference) {
        if i.lifetime.is_none() {
            i.lifetime = Some(self.lifetime.clone())
        }
    }
}

struct ReplaceCoroutineAwait {
    resume_type: syn::Type,
}

impl VisitMut for ReplaceCoroutineAwait {
    fn visit_expr_mut(&mut self, i: &mut syn::Expr) {
        match i {
            syn::Expr::Await(ei) => {
                assert!(ei.attrs.is_empty());

                let resume_type = self.resume_type.clone();
                let base = &ei.base;

                *i = syn::parse_quote! {
                    {
                        let mut __coroutine = #base;
                        let mut __response: #resume_type = Default::default();

                        loop {
                            use ::core::{pin::Pin, ops::{Coroutine, CoroutineState}};

                            match unsafe { Pin::new_unchecked(&mut __coroutine) }.resume(__response) {
                                CoroutineState::Yielded(__request) => __response = yield __request.into(),
                                CoroutineState::Complete(__result) => break __result,
                            }
                        }
                    }
                };
            }
            _ => syn::visit_mut::visit_expr_mut(self, i),
        }
    }
}

mod kw {
    syn::custom_keyword!(lifetime);
}

struct CoroutineInput {
    is_static: bool,

    yield_type: Option<syn::Type>,
    resume_type: Option<syn::Type>,
    lifetime: Option<syn::Lifetime>,
}

impl Parse for CoroutineInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut output = Self {
            is_static: false,
            yield_type: None,
            resume_type: None,
            lifetime: None,
        };

        {
            let lk = input.lookahead1();

            if lk.peek(Token![static]) {
                output.is_static = true;
                input.parse::<Token![static]>()?;
                if input.is_empty() {
                    return Ok(output);
                } else {
                    input.parse::<Token![,]>()?;
                    input.parse::<Token![yield]>()?;
                }
            } else if lk.peek(Token![yield]) {
            } else {
                return Err(lk.error());
            }
        }

        if input.is_empty() {
            return Ok(output);
        }

        output.yield_type = Some(input.parse::<syn::Type>()?);

        if input.is_empty() {
            return Ok(output);
        }

        input.parse::<Token![->]>()?;
        output.resume_type = Some(input.parse::<syn::Type>()?);

        if input.is_empty() {
            return Ok(output);
        }

        input.parse::<Token![,]>()?;
        input.parse::<kw::lifetime>()?;
        output.lifetime = Some(input.parse::<syn::Lifetime>()?);

        Ok(output)
    }
}

struct BareItemFn {
    attrs: Vec<syn::Attribute>,
    vis: syn::Visibility,
    sig: syn::Signature,
    block: TokenStream2,
}

impl Parse for BareItemFn {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Ok(Self {
            attrs: syn::Attribute::parse_outer(input)?,
            vis: syn::Visibility::parse(input)?,
            sig: syn::Signature::parse(input)?,
            block: input.parse::<TokenStream2>()?,
        })
    }
}

#[proc_macro_attribute]
pub fn generator(attr_ts: TokenStream, ts: TokenStream) -> TokenStream {
    let func_result = syn::parse::<BareItemFn>(ts.clone());
    let input_result = match attr_ts.is_empty() {
        true => Ok(None),
        false => syn::parse::<CoroutineInput>(attr_ts).map(Some),
    };

    if let Err(err) = &input_result {
        panic!("{err}");
    }

    if let (Ok(func), Ok(input)) = (func_result, input_result) {
        let unit_type: syn::Type = syn::parse_quote!(());

        let attrs = func.attrs;
        let vis = func.vis;
        let name = func.sig.ident;
        let mut generics = func.sig.generics;
        let mut args = func.sig.inputs;
        let return_type = match func.sig.output {
            syn::ReturnType::Default => &unit_type,
            syn::ReturnType::Type(_, ref tp) => tp,
        };

        let coro_lifetime = match input.as_ref().and_then(|x| x.lifetime.clone()) {
            Some(lt) => lt,
            None => {
                let lt = syn::Lifetime::new("'__coroutine", Span::call_site());
                generics.params.insert(0, syn::parse_quote!(#lt));
                lt
            }
        };

        {
            let mut ladder = LifetimeAdder {
                lifetime: coro_lifetime.clone(),
            };
            for arg in args.iter_mut() {
                match arg {
                    syn::FnArg::Receiver(recv) => {
                        if let Some((_, lifetime @ None)) = &mut recv.reference {
                            *lifetime = Some(coro_lifetime.clone())
                        }
                    }
                    syn::FnArg::Typed(pat) => ladder.visit_pat_type_mut(pat),
                }
            }
        }

        let (yield_type, resume_type) = {
            let opts = input
                .as_ref()
                .map(|x| (x.yield_type.clone(), x.resume_type.clone()))
                .unwrap_or_default();

            (
                opts.0.unwrap_or_else(|| unit_type.clone()),
                opts.1.unwrap_or_else(|| unit_type.clone()),
            )
        };

        let generic_params = generics.params;
        let where_clause = generics.where_clause;

        let new_body = if let Ok(mut block) = syn::parse2::<syn::Block>(func.block.clone()) {
            ReplaceCoroutineAwait {
                resume_type: resume_type.clone(),
            }
            .visit_block_mut(&mut block);

            let maybe_static = input
                .map(|x| {
                    if x.is_static {
                        quote!(static)
                    } else {
                        quote!()
                    }
                })
                .unwrap_or(quote!());

            quote!({
                #[coroutine] #maybe_static move |_: #resume_type| #block
            })
        } else {
            func.block
        };

        quote! {
            #(#attrs)*
            #vis fn #name<#generic_params>(#args) -> impl ::core::ops::Coroutine<
                #resume_type,
                Yield = #yield_type,
                Return = #return_type
            > + #coro_lifetime #where_clause #new_body
        }
        .into()
    } else {
        ts
    }
}
