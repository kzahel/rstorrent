//! Wrapping uTP sequence-number and timestamp arithmetic.

/// A uTP packet sequence number.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SequenceNumber(u16);

impl SequenceNumber {
    #[must_use]
    pub const fn new(value: u16) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u16 {
        self.0
    }

    #[must_use]
    pub const fn wrapping_add(self, amount: u16) -> Self {
        Self(self.0.wrapping_add(amount))
    }

    #[must_use]
    pub const fn wrapping_sub(self, amount: u16) -> Self {
        Self(self.0.wrapping_sub(amount))
    }

    /// Compare two wrapping values without inventing an order at half range.
    #[must_use]
    pub const fn relation_to(self, other: Self) -> SequenceRelation {
        let distance = self.0.wrapping_sub(other.0);
        match distance {
            0 => SequenceRelation::Equal,
            0x8000 => SequenceRelation::Ambiguous,
            1..=0x7fff => SequenceRelation::After(distance),
            _ => SequenceRelation::Before(other.0.wrapping_sub(self.0)),
        }
    }
}

impl From<u16> for SequenceNumber {
    fn from(value: u16) -> Self {
        Self::new(value)
    }
}

impl From<SequenceNumber> for u16 {
    fn from(value: SequenceNumber) -> Self {
        value.get()
    }
}

/// The relation of the left sequence number to the right sequence number.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SequenceRelation {
    Before(u16),
    Equal,
    After(u16),
    Ambiguous,
}

/// The wrapping 32-bit microsecond timestamp carried by uTP.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TimestampMicros(u32);

impl TimestampMicros {
    #[must_use]
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// Return elapsed wire-microseconds using u32 wrap semantics.
    #[must_use]
    pub const fn elapsed_since(self, earlier: Self) -> u32 {
        self.0.wrapping_sub(earlier.0)
    }
}

impl From<u32> for TimestampMicros {
    fn from(value: u32) -> Self {
        Self::new(value)
    }
}

impl From<TimestampMicros> for u32 {
    fn from(value: TimestampMicros) -> Self {
        value.get()
    }
}

#[cfg(test)]
mod tests {
    use super::{SequenceNumber, SequenceRelation, TimestampMicros};

    #[test]
    fn sequence_relation_is_explicit_across_wrap_and_half_range() {
        let end = SequenceNumber::new(u16::MAX);
        let zero = SequenceNumber::new(0);
        assert_eq!(zero.relation_to(end), SequenceRelation::After(1));
        assert_eq!(end.relation_to(zero), SequenceRelation::Before(1));
        assert_eq!(zero.relation_to(zero), SequenceRelation::Equal);
        assert_eq!(
            SequenceNumber::new(0x8000).relation_to(zero),
            SequenceRelation::Ambiguous
        );
        assert_eq!(
            zero.relation_to(SequenceNumber::new(0x8000)),
            SequenceRelation::Ambiguous
        );
    }

    #[test]
    fn sequence_arithmetic_wraps_without_panicking() {
        assert_eq!(SequenceNumber::new(u16::MAX).wrapping_add(1).get(), 0);
        assert_eq!(SequenceNumber::new(0).wrapping_sub(1).get(), u16::MAX);
    }

    #[test]
    fn timestamp_elapsed_uses_wire_wrap_semantics() {
        let earlier = TimestampMicros::new(u32::MAX - 4);
        let later = TimestampMicros::new(7);
        assert_eq!(later.elapsed_since(earlier), 12);
    }
}
