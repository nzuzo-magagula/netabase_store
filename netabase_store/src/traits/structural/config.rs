// @review [x]
//
// Reserved module for the planned TOML schema feature (rebuild the repository/definition/model tree
// from a TOML description + a companion macro). See the "TOML schema (future)" section in
// ARCHITECTURE.md for the intended round-trip contract.
//
// The previous `NetabaseConfig<R>` trait (two empty associated types, no implementors, no callers)
// was removed as dead code: it described nothing concrete and was never wired to
// `NetabaseStore::new(path)`. When the TOML feature is built, define the config/round-trip traits
// here alongside their concrete implementors and macro support, rather than as a bare stub.
