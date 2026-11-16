use super::*;

#[test]
fn every_schema_compiles() {
    for kind in KINDS {
        let v = validator(kind);
        assert_eq!(v.pointer(v.root()), "");
    }
}
