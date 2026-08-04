//! Canonical half-open range behavior.

use super::support::*;

#[test]
fn piece_ranges_do_not_expand_indices() {
    let mut pieces = vec![false; 70_005];
    pieces[65_536..70_000].fill(true);
    assert_eq!(
        ranges_from_pieces(&pieces),
        vec![IndexRange {
            start: 65_536,
            end_exclusive: 70_000
        }]
    );
}
