//! Things the unit system must refuse at compile time. trybuild compares the
//! compiler's actual error with the saved `.stderr`, so a snippet that fails
//! for some other reason (a typo, a renamed function) fails this test.
//! (Stable rustdoc ignores `compile_fail,E0308` codes, so doc tests alone
//! could pass for the wrong reason.)
#[test]
fn refused() {
    trybuild::TestCases::new().compile_fail("tests/refused/*.rs");
}
