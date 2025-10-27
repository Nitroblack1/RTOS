use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::Parse,
    parse::ParseStream,
    parse_macro_input,
    Ident,
    ItemFn,
    LitInt,
    LitStr,
    Token,
};

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
    let mut entry_fn = parse_macro_input!(input as ItemFn);
    let fn_name = entry_fn.sig.ident.clone();
    let fn_span = fn_name.span();

    // Parse macro arguments
    let args = if args.is_empty() {
        AppArgs { id: None, stack_size: None, name: None }
    } else {
        parse_macro_input!(args as AppArgs)
    };

    let app_id = args.id.expect("app macro requires 'id' parameter");
    let stack_size = args.stack_size.unwrap_or_else(|| syn::parse_str("512").unwrap());
    let app_name = args.name.unwrap_or_else(|| syn::parse_str(&format!("\"{}\"", fn_name)).unwrap());
    let fn_upper = fn_name.to_string().to_uppercase();
    let metadata_name = format_ident!("{}_METADATA", fn_upper);
    let stack_words_name = format_ident!("{}_STACK_WORDS", fn_upper);
    let stack_block_name = format_ident!("{}_STACK_BLOCK", fn_upper);
    let stack_static_name = format_ident!("{}_STACK", fn_upper);
    let stack_ptr_fn_name = format_ident!("{}_stack_ptr", fn_name);

    let stack_size_value = stack_size.base10_parse::<usize>().expect("stack_size must be a positive integer");
    // Enforce kernel minimum stack size (512 bytes)
    let min_stack_bytes = 512;
    let requested_bytes = if stack_size_value < min_stack_bytes {
        min_stack_bytes
    } else {
        stack_size_value
    };
    let aligned_bytes = ((requested_bytes + 7) / 8) * 8; // Align to 8 bytes
    let stack_words_value = (aligned_bytes + 3) / 4; // Convert bytes to 32-bit words

    let stack_words_lit = syn::LitInt::new(&format!("{}usize", stack_words_value), fn_span);
    let stack_size_bytes_u32 = syn::LitInt::new(&format!("{}u32", aligned_bytes), fn_span);

    // Force function to use C ABI so scheduler can call it safely
    entry_fn.sig.abi = Some(syn::Abi {
        extern_token: syn::token::Extern { span: fn_span },
        name: Some(LitStr::new("C", fn_span)),
    });
    entry_fn.attrs.retain(|attr| !attr.path().is_ident("app"));

    let expanded = quote! {
        const #stack_words_name: usize = #stack_words_lit;

        #[repr(align(8))]
        struct #stack_block_name([u32; #stack_words_name]);

        #[unsafe(link_section = ".app_stacks")]
        #[used]
        static mut #stack_static_name: #stack_block_name = #stack_block_name([0; #stack_words_name]);

        #[inline(never)]
        unsafe extern "C" fn #stack_ptr_fn_name() -> usize {
            #stack_static_name.0.as_mut_ptr() as usize
        }

        #[unsafe(link_section = ".app_registry")]
        #[used]
        static #metadata_name: crate::AppMetadata = crate::AppMetadata {
            id: #app_id,
            name: #app_name,
            entry: 0,
            entry_fn: Some(#fn_name),
            stack_ptr: 0,
            stack_size: #stack_size_bytes_u32,
            stack_ptr_fn: Some(#stack_ptr_fn_name),
        };

        #entry_fn
    };

    TokenStream::from(expanded)
}
