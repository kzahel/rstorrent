//! Independent public-API regression for RUSTSEC-2024-0429.
//! Native Linux CI also runs this target with --release: optimization exposed
//! the invalid immutable C out-pointer in the original dependency.
#![cfg(target_os = "linux")]

use glib::prelude::*;

#[test]
fn every_string_iterator_entry_point_returns_borrowed_content() {
    let strings = ["", "alpha", "Grüße 🌍", "last"];
    let variant = strings.to_variant();
    assert_eq!(
        variant.array_iter_str().unwrap().collect::<Vec<_>>(),
        strings
    );
    assert_eq!(variant.array_iter_str().unwrap().next(), Some(""));
    assert_eq!(variant.array_iter_str().unwrap().nth(2), Some("Grüße 🌍"));
    assert_eq!(variant.array_iter_str().unwrap().last(), Some("last"));
    assert_eq!(variant.array_iter_str().unwrap().next_back(), Some("last"));
    assert_eq!(
        variant.array_iter_str().unwrap().nth_back(1),
        Some("Grüße 🌍")
    );
    assert_eq!(
        variant.array_iter_str().unwrap().rev().collect::<Vec<_>>(),
        strings.into_iter().rev().collect::<Vec<_>>()
    );
}

#[test]
fn mixed_direction_exhaustion_preserves_bounds_and_borrowed_values() {
    let variant = ["first", "middle", "tail"].to_variant();
    let mut iter = variant.array_iter_str().unwrap();
    let first = iter.next().unwrap();
    assert_eq!(iter.len(), 2);
    assert_eq!(iter.size_hint(), (2, Some(2)));
    assert_eq!(iter.next_back(), Some("tail"));
    assert_eq!(iter.next(), Some("middle"));
    assert_eq!(iter.len(), 0);
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_back(), None);
    assert_eq!(iter.nth(usize::MAX), None);
    assert_eq!(iter.nth_back(usize::MAX), None);
    assert_eq!(iter.last(), None);
    assert_eq!(first, "first");
}

#[test]
fn empty_singleton_and_oversized_skips_are_fused() {
    for values in [Vec::<&str>::new(), vec!["only"]] {
        let variant = values.to_variant();
        assert_eq!(
            variant.array_iter_str().unwrap().collect::<Vec<_>>(),
            values
        );
        assert_eq!(
            variant.array_iter_str().unwrap().last(),
            values.last().copied()
        );
        for n in [values.len(), usize::MAX] {
            let mut forward = variant.array_iter_str().unwrap();
            assert_eq!(forward.nth(n), None);
            assert_eq!(forward.next_back(), None);
            let mut backward = variant.array_iter_str().unwrap();
            assert_eq!(backward.nth_back(n), None);
            assert_eq!(backward.next(), None);
        }
    }
    assert!(42u32.to_variant().array_iter_str().is_err());
}
