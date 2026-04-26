//! Path-enum addressing: the single source of truth for routing addresses and physical
//! table names across the schema tree.
//!
//! A [`PathEnum`] is a node in a nested addressing tree. Non-leaf variants are uniform
//! `Variant(Child)` where `Child` is itself a `PathEnum` (a deeper node), and **leaves are
//! ZSTs** that carry the physical name of the thing being addressed. Because every variant is
//! `Variant(Child)`, the whole surface (enum decl + the impls) is generated from one
//! repetition by [`netabase_path_enum!`]; leaves come from [`netabase_path_leaf!`].
//!
//! Two string surfaces are exposed so this one mechanism subsumes both addressing and naming:
//! - [`PathEnum::static_name`] — a `&'static str`, the physical table name (always a
//!   compile-time constant, delegated down to the leaf's literal).
//! - [`PathEnum::write_path`] / [`PathEnum::from_path`] — the composed full path
//!   (`"Secondary/username"`) for query parsing and cross-store string keys, with a round-trip
//!   guarantee. `write_path` targets any `core::fmt::Write` so the core stays heap-free; the
//!   `String`-returning [`PathEnum::to_path`] convenience is std-only.

/// Separator between path segments in the composed path form.
pub const PATH_SEP: char = '/';

/// Error parsing a string path back into a [`PathEnum`]. Heap-free: carries
/// the type that failed to parse; positional detail is intentionally omitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathParseError {
    /// Name of the path-enum type that rejected the input.
    pub type_name: &'static str,
}

impl core::fmt::Display for PathParseError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "path parse error in {}", self.type_name)
    }
}

impl core::error::Error for PathParseError {}

/// A node in the addressing tree. See the module docs.
pub trait PathEnum: Sized {
    /// Whether this node is a leaf (contributes no path segment).
    const IS_LEAF: bool;

    /// The physical table name this address resolves to. Always a compile-time literal
    /// (delegated down to the leaf), so it is usable as a backend table name.
    fn static_name(&self) -> &'static str;

    /// Write the composed full path from this node down (segments joined by [`PATH_SEP`]).
    /// A leaf contributes no segment, so a single-level address like `Primary` writes as
    /// `"Primary"`.
    fn write_path(&self, out: &mut impl core::fmt::Write) -> core::fmt::Result;

    /// Parse a full path (as produced by [`write_path`](PathEnum::write_path)) back into
    /// this node.
    fn from_path(path: &str) -> Result<Self, PathParseError>;

    /// The composed, owned full path (std convenience over `write_path`).
    #[cfg(feature = "std")]
    fn to_path(&self) -> String {
        let mut out = String::new();
        self.write_path(&mut out)
            .expect("fmt::Write for String is infallible");
        out
    }
}

/// Define a ZST leaf of the addressing tree. The leaf carries the physical table name and
/// terminates a path.
///
/// ```ignore
/// netabase_path_leaf! { pub struct UserBlobLeaf => "User_Blob" }
/// ```
#[macro_export]
macro_rules! netabase_path_leaf {
    ($(#[$m:meta])* $vis:vis struct $name:ident => $phys:expr $(,)?) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
        $vis struct $name;

        impl $crate::traits::structural::addressing::PathEnum for $name {
            const IS_LEAF: bool = true;

            fn static_name(&self) -> &'static str { $phys }

            fn write_path(
                &self,
                _out: &mut impl ::core::fmt::Write,
            ) -> ::core::fmt::Result {
                ::core::result::Result::Ok(())
            }

            fn from_path(
                path: &str,
            ) -> ::core::result::Result<Self, $crate::traits::structural::addressing::PathParseError> {
                if path.is_empty() {
                    ::core::result::Result::Ok($name)
                } else {
                    ::core::result::Result::Err(
                        $crate::traits::structural::addressing::PathParseError {
                            type_name: ::core::stringify!($name),
                        },
                    )
                }
            }
        }
    };
}

/// Define a non-leaf [`PathEnum`] node. Every variant nests a child `PathEnum` (a deeper node
/// or a [`netabase_path_leaf!`] ZST), so the enum decl and all impls are generated from one
/// repetition and can never drift.
///
/// ```ignore
/// netabase_path_enum! {
///     pub enum UserTableName {
///         Primary(UserPrimaryLeaf),
///         Secondary(UserSecondaryTable),
///         Blob(UserBlobLeaf),
///     }
/// }
/// ```
#[macro_export]
macro_rules! netabase_path_enum {
    (
        $(#[$m:meta])*
        $vis:vis enum $name:ident {
            $( $variant:ident ( $child:ty ) ),* $(,)?
        }
    ) => {
        $(#[$m])*
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        $vis enum $name {
            $( $variant($child), )*
        }

        impl $crate::traits::structural::addressing::PathEnum for $name {
            const IS_LEAF: bool = false;

            fn static_name(&self) -> &'static str {
                match self {
                    $( Self::$variant(__c) =>
                        $crate::traits::structural::addressing::PathEnum::static_name(__c), )*
                }
            }

            fn write_path(
                &self,
                out: &mut impl ::core::fmt::Write,
            ) -> ::core::fmt::Result {
                match self {
                    $( Self::$variant(__c) => {
                        out.write_str(::core::stringify!($variant))?;
                        if !<$child as $crate::traits::structural::addressing::PathEnum>::IS_LEAF {
                            out.write_char(
                                $crate::traits::structural::addressing::PATH_SEP,
                            )?;
                            $crate::traits::structural::addressing::PathEnum::write_path(
                                __c, out,
                            )?;
                        }
                        ::core::result::Result::Ok(())
                    } )*
                }
            }

            fn from_path(
                path: &str,
            ) -> ::core::result::Result<Self, $crate::traits::structural::addressing::PathParseError> {
                let (__head, __tail) = match path.split_once(
                    $crate::traits::structural::addressing::PATH_SEP,
                ) {
                    ::core::option::Option::Some((__h, __t)) => (__h, __t),
                    ::core::option::Option::None => (path, ""),
                };
                $( if __head == ::core::stringify!($variant) {
                    return ::core::result::Result::Ok(Self::$variant(
                        <$child as $crate::traits::structural::addressing::PathEnum>::from_path(__tail)?,
                    ));
                } )*
                ::core::result::Result::Err(
                    $crate::traits::structural::addressing::PathParseError {
                        type_name: ::core::stringify!($name),
                    },
                )
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::PathEnum;

    netabase_path_leaf! { pub struct UserPrimaryLeaf => "User" }
    netabase_path_leaf! { pub struct UserBlobLeaf => "User_Blob" }
    netabase_path_leaf! { pub struct UserNameSecondaryLeaf => "User_Secondary_name" }
    netabase_path_leaf! { pub struct UserEmailSecondaryLeaf => "User_Secondary_email" }

    netabase_path_enum! {
        pub enum UserSecondaryTable {
            Name(UserNameSecondaryLeaf),
            Email(UserEmailSecondaryLeaf),
        }
    }

    netabase_path_enum! {
        pub enum UserTableName {
            Primary(UserPrimaryLeaf),
            Secondary(UserSecondaryTable),
            Blob(UserBlobLeaf),
        }
    }

    #[test]
    fn static_name_delegates_to_leaf() {
        assert_eq!(UserTableName::Primary(UserPrimaryLeaf).static_name(), "User");
        assert_eq!(
            UserTableName::Secondary(UserSecondaryTable::Name(UserNameSecondaryLeaf))
                .static_name(),
            "User_Secondary_name"
        );
        assert_eq!(UserTableName::Blob(UserBlobLeaf).static_name(), "User_Blob");
    }

    #[test]
    fn to_path_composes_segments() {
        assert_eq!(UserTableName::Primary(UserPrimaryLeaf).to_path(), "Primary");
        assert_eq!(
            UserTableName::Secondary(UserSecondaryTable::Email(UserEmailSecondaryLeaf)).to_path(),
            "Secondary/Email"
        );
    }

    #[test]
    fn from_path_round_trips() {
        for addr in [
            UserTableName::Primary(UserPrimaryLeaf),
            UserTableName::Secondary(UserSecondaryTable::Name(UserNameSecondaryLeaf)),
            UserTableName::Secondary(UserSecondaryTable::Email(UserEmailSecondaryLeaf)),
            UserTableName::Blob(UserBlobLeaf),
        ] {
            assert_eq!(UserTableName::from_path(&addr.to_path()).unwrap(), addr);
        }
    }

    #[test]
    fn from_path_rejects_unknown_segments() {
        assert!(UserTableName::from_path("Nope").is_err());
        assert!(UserTableName::from_path("Secondary/Nope").is_err());
        assert!(UserTableName::from_path("Primary/Extra").is_err());
    }
}
