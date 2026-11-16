use std::cmp::Ordering;
use std::sync::OnceLock;

use icu_collator::options::CollatorOptions;
use icu_collator::{Collator, CollatorBorrowed};

/// `a < b` on JavaScript strings: by UTF-16 code units, the order of `sort()` without a comparator
pub fn cmp_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `list.sort()` on strings
pub fn sort_utf16(list: &mut [String]) {
    list.sort_by(|a, b| cmp_utf16(a, b));
}

fn collator() -> &'static CollatorBorrowed<'static> {
    static C: OnceLock<CollatorBorrowed<'static>> = OnceLock::new();
    C.get_or_init(|| {
        Collator::try_new(Default::default(), CollatorOptions::default()).expect("the root collation is compiled in")
    })
}

/// `a.localeCompare(b)`: the Unicode collation of the root locale at tertiary strength, which is the order Node gives
/// under the C, POSIX and en locales (a CI runner's)
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    collator().compare(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collation_as_node_orders_under_en() {
        // node -e "console.log(['b','B','a','_x','-x','.x','/x','A','x/y','x-y','x.y','ı','i','I'].sort((a,b)=>a.localeCompare(b)).join(' '))"
        let mut v = ["b", "B", "a", "_x", "-x", ".x", "/x", "A", "x/y", "x-y", "x.y", "ı", "i", "I"];
        v.sort_by(|a, b| locale_compare(a, b));
        assert_eq!(v.join(" "), "_x -x .x /x a A b B i I ı x-y x.y x/y");
    }

    #[test]
    fn code_unit_order() {
        assert_eq!(cmp_utf16("\u{FF5E}", "😀"), Ordering::Greater);
        assert_eq!(cmp_utf16("B", "a"), Ordering::Less);
    }
}
