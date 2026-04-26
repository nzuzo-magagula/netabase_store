// @review [ ]
#[test]
fn macro_fixtures() {
    let t = trybuild::TestCases::new();
    t.pass("tests/fixtures/pass/*.rs");
    t.compile_fail("tests/fixtures/fail/*.rs");
}
