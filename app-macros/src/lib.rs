use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ItemFn, parse::Parse, parse::ParseStream, Token, Ident, LitInt, LitStr};

struct AppArgs {
    id: Option<LitInt>,
    stack_size: Option<LitInt>,
    name: Option<LitStr>,
}

impl Parse for AppArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut id = None;
        let mut stack_size = None;
        let mut name = None;

        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;

            match key.to_string().as_str() {
                "id" => {
                    id = Some(input.parse()?);
                },
                "stack_size" => {
                    stack_size = Some(input.parse()?);
                },
                "name" => {
                    name = Some(input.parse()?);
                },
                _ => {
                    return Err(syn::Error::new(key.span(), "Unknown parameter"));
                }
            }

            // Parse optional comma
            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(AppArgs { id, stack_size, name })
    }
}

/// Application entry point attribute.
///
/// Usage:
/// ```rust
/// #[app(id = 0, stack_size = 1024, name = "task0")]
/// fn my_app() -> ! {
///     loop { /* app code */ }
/// }
/// ```
#[proc_macro_attribute]
pub fn app(args: TokenStream, input: TokenStream) -> TokenStream {
    let input_fn = parse_macro_input!(input as ItemFn);
    let fn_name = &input_fn.sig.ident;
    let fn_vis = &input_fn.vis;
    let fn_block = &input_fn.block;
    let _fn_inputs = &input_fn.sig.inputs; // Unused but may be needed for future enhancements
    let _fn_output = &input_fn.sig.output; // Unused but may be needed for future enhancements

    // Parse macro arguments
    let args = if args.is_empty() {
        AppArgs { id: None, stack_size: None, name: None }
    } else {
        parse_macro_input!(args as AppArgs)
    };

    let app_id = args.id.expect("app macro requires 'id' parameter");
    let stack_size = args.stack_size.unwrap_or_else(|| syn::parse_str("512").unwrap());
    let app_name = args.name.unwrap_or_else(|| syn::parse_str(&format!("\"{}\"", fn_name)).unwrap());

    let entry_fn_name = syn::Ident::new(&format!("{}_entry", fn_name), fn_name.span());
    let metadata_name = syn::Ident::new(&format!("{}_METADATA", fn_name.to_string().to_uppercase()), fn_name.span());
    let _stack_name = syn::Ident::new(&format!("{}_STACK", fn_name.to_string().to_uppercase()), fn_name.span()); // Future use
    let _stack_ptr_fn_name = syn::Ident::new(&format!("{}_stack_ptr", fn_name), fn_name.span()); // Future use

    let expanded = quote! {
        // Original function becomes the entry point - remove the original function signature issues
        #[export_name = stringify!(#entry_fn_name)]
        pub extern "C" fn #entry_fn_name() -> ! #fn_block

        // App metadata stored in special linker section for automatic collection
        #[link_section = ".apps"]
        #[used]
        static #metadata_name: crate::AppMetadata = crate::AppMetadata {
            id: #app_id,
            name: #app_name,
            entry: 0, // Will be resolved at runtime by finding the symbol
            entry_fn: Some(#entry_fn_name), // Direct function pointer!
            stack_ptr: 0, // Stack will be allocated by scheduler
            stack_size: #stack_size,
            stack_ptr_fn: None, // Not used with static allocation approach
        };

        // Keep the original function name for user reference (remove parameters to avoid issues)
        #fn_vis fn #fn_name() -> ! {
            #entry_fn_name()
        }
    };

    TokenStream::from(expanded)
}