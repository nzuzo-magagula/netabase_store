// @review [ ]
pub fn format_variant_ident(ident: &syn::Ident) -> syn::Ident {
    let s = ident.to_string();
    let mut result = String::with_capacity(s.len());
    let mut capitalize_next = true;

    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    quote::format_ident!("{}", result)
}

use quote::quote;
use syn::Type;

/// Generated impls for a key newtype `struct Name(pub Inner)` over a fixed
/// inner type: ordered key encoding (delegating to the inner type) plus a
/// fixed-width rkyv value form (delegating Archive to the inner type), so the
/// newtype can serve as both a table key and a table value.
pub fn key_newtype_impls(name: &syn::Ident, inner: &Type) -> proc_macro2::TokenStream {
    let archived = quote::format_ident!("Archived{}", name);
    quote! {
        // Derive rkyv directly so the newtype gets its OWN archived type
        // (`Archived{name}`), avoiding the orphan/coherence conflict that a
        // delegating `Deserialize for <Inner>::Archived` would hit against
        // rkyv's `With` blanket impl.
        #[derive(
            Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Default, Hash,
            ::netabase_store::reexports::rkyv::Archive,
            ::netabase_store::reexports::rkyv::Serialize,
            ::netabase_store::reexports::rkyv::Deserialize,
        )]
        #[rkyv(derive(Debug))]
        pub struct #name(pub #inner);

        // SAFETY: `#[repr(C)]` (rkyv derive) single-field wrapper over the
        // inner archived type, which is itself NoUndef; no padding.
        unsafe impl ::netabase_store::reexports::rkyv::traits::NoUndef for #archived {}

        impl #name {
            pub fn new(val: #inner) -> Self { Self(val) }
        }

        impl ::core::convert::From<#inner> for #name {
            fn from(val: #inner) -> Self { Self(val) }
        }

        impl ::netabase_store::keys::ordered::OrderedKeyEncoding for #name {
            const MAX_ENCODED_LEN: usize =
                <#inner as ::netabase_store::keys::ordered::OrderedKeyEncoding>::MAX_ENCODED_LEN;

            fn encode_into(
                &self,
                out: &mut [u8],
            ) -> ::core::result::Result<usize, ::netabase_store::keys::ordered::KeyCodecError> {
                ::netabase_store::keys::ordered::OrderedKeyEncoding::encode_into(&self.0, out)
            }

            fn decode(
                bytes: &[u8],
            ) -> ::core::result::Result<(Self, usize), ::netabase_store::keys::ordered::KeyCodecError> {
                let (inner, consumed) =
                    <#inner as ::netabase_store::keys::ordered::OrderedKeyEncoding>::decode(bytes)?;
                ::core::result::Result::Ok((Self(inner), consumed))
            }
        }
    }
}
/// Generated `OrderedKeyEncoding` for a key enum: a leading tag byte in
/// variant-declaration order, then the variant payload's own encoding (or
/// nothing for unit variants). Byte order therefore groups entries by
/// variant — a range bounded by one variant is a contiguous category scan.
///
/// `variants` is (ident, Some(payload type) | None for unit variants).
pub fn key_enum_ordered_encoding(
    name: &syn::Ident,
    variants: &[(syn::Ident, Option<proc_macro2::TokenStream>)],
) -> proc_macro2::TokenStream {
    let max_terms = variants.iter().map(|(_, payload)| match payload {
        Some(ty) => quote! {
            __max = ::netabase_store::keys::ordered::const_max(
                __max,
                <#ty as ::netabase_store::keys::ordered::OrderedKeyEncoding>::MAX_ENCODED_LEN,
            );
        },
        None => quote! {},
    });

    let encode_arms = variants.iter().enumerate().map(|(i, (v, payload))| {
        let tag = i as u8;
        match payload {
            Some(_) => quote! {
                Self::#v(__payload) => {
                    *out.first_mut().ok_or(::netabase_store::keys::ordered::KeyCodecError::BufferTooSmall)? = #tag;
                    let __n = ::netabase_store::keys::ordered::OrderedKeyEncoding::encode_into(__payload, &mut out[1..])?;
                    ::core::result::Result::Ok(1 + __n)
                }
            },
            None => quote! {
                Self::#v => {
                    *out.first_mut().ok_or(::netabase_store::keys::ordered::KeyCodecError::BufferTooSmall)? = #tag;
                    ::core::result::Result::Ok(1)
                }
            },
        }
    });

    let decode_arms = variants.iter().enumerate().map(|(i, (v, payload))| {
        let tag = i as u8;
        match payload {
            Some(ty) => quote! {
                #tag => {
                    let (__payload, __n) =
                        <#ty as ::netabase_store::keys::ordered::OrderedKeyEncoding>::decode(&bytes[1..])?;
                    ::core::result::Result::Ok((Self::#v(__payload), 1 + __n))
                }
            },
            None => quote! {
                #tag => ::core::result::Result::Ok((Self::#v, 1)),
            },
        }
    });

    // An uninhabited key enum (a definition/repository with no children, or a
    // model with no fields in this category) has no values to encode; its
    // accessors are unreachable. `match *self {}` is exhaustive on the
    // uninhabited type (unlike `match self {}` on the always-inhabited `&Self`).
    let (encode_body, decode_body) = if variants.is_empty() {
        (
            quote! { match *self {} },
            quote! {
                let _ = bytes;
                ::core::result::Result::Err(
                    ::netabase_store::keys::ordered::KeyCodecError::Malformed,
                )
            },
        )
    } else {
        (
            quote! {
                match self {
                    #(#encode_arms)*
                }
            },
            quote! {
                let __tag = *bytes
                    .first()
                    .ok_or(::netabase_store::keys::ordered::KeyCodecError::Truncated)?;
                match __tag {
                    #(#decode_arms)*
                    __other => ::core::result::Result::Err(
                        ::netabase_store::keys::ordered::KeyCodecError::UnknownTag(__other),
                    ),
                }
            },
        )
    };

    quote! {
        impl ::netabase_store::keys::ordered::OrderedKeyEncoding for #name {
            const MAX_ENCODED_LEN: usize = {
                let mut __max = 0usize;
                #(#max_terms)*
                1 + __max
            };

            fn encode_into(
                &self,
                out: &mut [u8],
            ) -> ::core::result::Result<usize, ::netabase_store::keys::ordered::KeyCodecError> {
                let _ = out;
                #encode_body
            }

            fn decode(
                bytes: &[u8],
            ) -> ::core::result::Result<(Self, usize), ::netabase_store::keys::ordered::KeyCodecError> {
                #decode_body
            }
        }
    }
}
