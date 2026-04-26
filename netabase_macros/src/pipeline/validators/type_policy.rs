//! The model field type policy: heap-allocated / dynamically-sized std types
//! are rejected at macro time with a diagnostic naming the fixed-capacity
//! replacement.
//!
//! This is the *good-error* layer; the actual enforcement is the
//! `StoreValue`/`OrderedKeyEncoding` bounds on generated code, which any
//! unknown non-fixed type will fail with an ordinary rustc error.

use syn::{GenericArgument, PathArguments, Type};

/// Returns the replacement hint for a denied type name, if it is denied.
fn replacement_for(ident: &str) -> Option<&'static str> {
    Some(match ident {
        "String" => "`String` is heap-allocated; use `NbString<N>` from netabase_arena::fixed",
        "Vec" => "`Vec<T>` is heap-allocated; use `NbVec<T, N>` from netabase_arena::fixed",
        "VecDeque" => "`VecDeque<T>` is heap-allocated; use `NbVec<T, N>` from netabase_arena::fixed",
        "BTreeMap" | "HashMap" => {
            "this map is heap-allocated; use `NbMap<K, V, N>` from netabase_arena::fixed"
        }
        "BTreeSet" | "HashSet" => {
            "this set is heap-allocated; use `NbVec<T, N>` (kept sorted) from netabase_arena::fixed"
        }
        "Option" => {
            "`Option<T>`'s archived form has undefined bytes when `None`; use `NbOption<T>` from netabase_arena::fixed"
        }
        "Box" | "Rc" | "Arc" => "smart pointers are heap-allocated; store the value inline",
        "Cow" => "`Cow` borrows or allocates; store the owned fixed-capacity form inline",
        "PathBuf" | "OsString" | "CString" => {
            "this string type is heap-allocated; use `NbString<N>` from netabase_arena::fixed"
        }
        "str" => "use `NbString<N>` from netabase_arena::fixed",
        _ => return None,
    })
}

/// Check one field type. Recurses into generic arguments so `NbVec<String, 4>`
/// is also caught. Errors carry the span of the offending type.
pub fn check_field_type(ty: &Type) -> syn::Result<()> {
    match ty {
        Type::Path(tp) => {
            let Some(last) = tp.path.segments.last() else {
                return Ok(());
            };
            let name = last.ident.to_string();
            if let Some(hint) = replacement_for(&name) {
                return Err(syn::Error::new_spanned(
                    ty,
                    format!("netabase model fields must be fixed-width: {hint}"),
                ));
            }
            if let PathArguments::AngleBracketed(args) = &last.arguments {
                for arg in &args.args {
                    if let GenericArgument::Type(inner) = arg {
                        check_field_type(inner)?;
                    }
                }
            }
            Ok(())
        }
        Type::Reference(_) => Err(syn::Error::new_spanned(
            ty,
            "netabase model fields must be owned fixed-width values, not references",
        )),
        Type::Slice(_) => Err(syn::Error::new_spanned(
            ty,
            "netabase model fields must be fixed-width: use `NbVec<T, N>` or `[T; N]`",
        )),
        Type::Array(arr) => check_field_type(&arr.elem),
        Type::Tuple(tup) => {
            for elem in &tup.elems {
                check_field_type(elem)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::check_field_type;
    use syn::parse_quote;

    #[test]
    fn denies_dynamic_types_with_replacement_hint() {
        for (ty, hint) in [
            (parse_quote!(String), "NbString"),
            (parse_quote!(Vec<u8>), "NbVec"),
            (parse_quote!(std::collections::BTreeMap<String, u32>), "NbMap"),
            (parse_quote!(Option<u32>), "NbOption"),
            (parse_quote!(Box<u32>), "inline"),
        ] {
            let ty: syn::Type = ty;
            let err = check_field_type(&ty).unwrap_err().to_string();
            assert!(err.contains(hint), "{err} should mention {hint}");
        }
    }

    #[test]
    fn denies_nested_dynamic_types() {
        let ty: syn::Type = parse_quote!(netabase_arena::fixed::NbVec<String, 4>);
        assert!(check_field_type(&ty).is_err());
        let ty: syn::Type = parse_quote!((u32, Vec<u8>));
        assert!(check_field_type(&ty).is_err());
    }

    #[test]
    fn allows_fixed_types() {
        for ty in [
            parse_quote!(u64),
            parse_quote!(bool),
            parse_quote!([u8; 32]),
            parse_quote!(netabase_arena::fixed::NbString<16>),
            parse_quote!(NbVec<u32, 8>),
            parse_quote!(NbMap<NbString<8>, u64, 4>),
            parse_quote!(NbOption<u128>),
        ] {
            let ty: syn::Type = ty;
            assert!(check_field_type(&ty).is_ok(), "{:?}", quote::quote!(#ty).to_string());
        }
    }
}
