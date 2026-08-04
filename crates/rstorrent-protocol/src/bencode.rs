use std::error::Error;
use std::fmt;
use std::ops::Range;

pub const MAX_BENCODE_INPUT_LENGTH: usize = 1024 * 1024;
pub const MAX_BENCODE_DECODED_ITEMS: usize = 1_000_000;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub max_input_length: usize,
    pub max_string_length: usize,
    pub max_decoded_items: usize,
    pub max_depth: usize,
    pub max_collection_entries: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_length: MAX_BENCODE_INPUT_LENGTH,
            max_string_length: 512 * 1024,
            max_decoded_items: MAX_BENCODE_DECODED_ITEMS,
            max_depth: 32,
            max_collection_entries: 4096,
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Node<'a> {
    pub value: Value<'a>,
    pub span: Range<usize>,
}

#[derive(Debug, PartialEq)]
pub enum Value<'a> {
    Integer(i64),
    Bytes(&'a [u8]),
    List(Vec<Node<'a>>),
    Dictionary(Vec<DictionaryEntry<'a>>),
}

#[derive(Debug, PartialEq)]
pub struct DictionaryEntry<'a> {
    pub key: &'a [u8],
    pub value: Node<'a>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    InputTooLarge {
        length: usize,
        maximum: usize,
    },
    UnexpectedEnd {
        position: usize,
    },
    InvalidToken {
        position: usize,
    },
    InvalidInteger {
        position: usize,
    },
    IntegerOverflow {
        position: usize,
    },
    InvalidStringLength {
        position: usize,
    },
    StringTooLarge {
        position: usize,
        length: usize,
        maximum: usize,
    },
    NestingTooDeep {
        position: usize,
        maximum: usize,
    },
    CollectionTooLarge {
        position: usize,
        maximum: usize,
    },
    TooManyDecodedItems {
        position: usize,
        maximum: usize,
    },
    DictionaryKeysNotStrictlySorted {
        position: usize,
    },
    TrailingData {
        position: usize,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLarge { length, maximum } => {
                write!(
                    formatter,
                    "bencode input length {length} exceeds limit {maximum}"
                )
            }
            Self::UnexpectedEnd { position } => {
                write!(formatter, "truncated bencode at byte {position}")
            }
            Self::InvalidToken { position } => {
                write!(formatter, "invalid bencode token at byte {position}")
            }
            Self::InvalidInteger { position } => {
                write!(formatter, "invalid bencode integer at byte {position}")
            }
            Self::IntegerOverflow { position } => {
                write!(
                    formatter,
                    "bencode integer overflows i64 at byte {position}"
                )
            }
            Self::InvalidStringLength { position } => {
                write!(
                    formatter,
                    "invalid bencode byte-string length at byte {position}"
                )
            }
            Self::StringTooLarge {
                position,
                length,
                maximum,
            } => write!(
                formatter,
                "bencode byte string at byte {position} has length {length}, limit {maximum}"
            ),
            Self::NestingTooDeep { position, maximum } => write!(
                formatter,
                "bencode nesting at byte {position} exceeds depth limit {maximum}"
            ),
            Self::CollectionTooLarge { position, maximum } => write!(
                formatter,
                "bencode collection at byte {position} exceeds entry limit {maximum}"
            ),
            Self::TooManyDecodedItems { position, maximum } => write!(
                formatter,
                "bencode item at byte {position} exceeds decoded-item limit {maximum}"
            ),
            Self::DictionaryKeysNotStrictlySorted { position } => write!(
                formatter,
                "bencode dictionary keys are not strictly sorted at byte {position}"
            ),
            Self::TrailingData { position } => {
                write!(
                    formatter,
                    "trailing data after bencode value at byte {position}"
                )
            }
        }
    }
}

impl Error for ParseError {}

pub fn parse(input: &[u8]) -> Result<Node<'_>, ParseError> {
    parse_with_limits(input, Limits::default())
}

pub fn parse_with_limits(input: &[u8], limits: Limits) -> Result<Node<'_>, ParseError> {
    let (node, consumed) = parse_prefix_with_limits(input, limits)?;
    if consumed != input.len() {
        return Err(ParseError::TrailingData { position: consumed });
    }
    Ok(node)
}

/// Parse bounded bencode while accepting out-of-order dictionary keys.
///
/// Some wire protocols have widely deployed implementations that emit
/// dictionaries out of canonical order. Duplicate keys remain rejected and
/// the returned entries are sorted so field lookup is deterministic. Metainfo
/// callers should continue using [`parse_with_limits`].
pub fn parse_with_limits_permissive_dictionaries(
    input: &[u8],
    limits: Limits,
) -> Result<Node<'_>, ParseError> {
    let (node, consumed) = parse_prefix_with_dictionary_order(input, limits, true)?;
    if consumed != input.len() {
        return Err(ParseError::TrailingData { position: consumed });
    }
    Ok(node)
}

pub fn parse_prefix(input: &[u8]) -> Result<(Node<'_>, usize), ParseError> {
    parse_prefix_with_limits(input, Limits::default())
}

pub fn parse_prefix_with_limits(
    input: &[u8],
    limits: Limits,
) -> Result<(Node<'_>, usize), ParseError> {
    parse_prefix_with_dictionary_order(input, limits, false)
}

fn parse_prefix_with_dictionary_order(
    input: &[u8],
    limits: Limits,
    allow_unsorted_dictionaries: bool,
) -> Result<(Node<'_>, usize), ParseError> {
    if input.len() > limits.max_input_length {
        return Err(ParseError::InputTooLarge {
            length: input.len(),
            maximum: limits.max_input_length,
        });
    }

    let mut parser = Parser {
        input,
        position: 0,
        decoded_items: 0,
        limits,
        allow_unsorted_dictionaries,
    };
    let node = parser.parse_value(0)?;
    Ok((node, parser.position))
}

struct Parser<'a> {
    input: &'a [u8],
    position: usize,
    decoded_items: usize,
    limits: Limits,
    allow_unsorted_dictionaries: bool,
}

impl<'a> Parser<'a> {
    fn parse_value(&mut self, depth: usize) -> Result<Node<'a>, ParseError> {
        if depth > self.limits.max_depth {
            return Err(ParseError::NestingTooDeep {
                position: self.position,
                maximum: self.limits.max_depth,
            });
        }

        let start = self.position;
        let token = self.peek()?;
        if !matches!(token, b'i' | b'l' | b'd' | b'0'..=b'9') {
            return Err(ParseError::InvalidToken { position: start });
        }
        self.consume_decoded_item()?;
        let value = match token {
            b'i' => self.parse_integer()?,
            b'l' => self.parse_list(depth)?,
            b'd' => self.parse_dictionary(depth)?,
            b'0'..=b'9' => Value::Bytes(self.parse_bytes()?),
            _ => unreachable!("token was validated above"),
        };
        Ok(Node {
            value,
            span: start..self.position,
        })
    }

    fn parse_integer(&mut self) -> Result<Value<'a>, ParseError> {
        let start = self.position;
        self.position += 1;
        let digits_start = self.position;
        while self.peek()? != b'e' {
            self.position += 1;
        }
        let digits = &self.input[digits_start..self.position];
        self.position += 1;

        let unsigned = digits.strip_prefix(b"-").unwrap_or(digits);
        let negative = unsigned.len() != digits.len();
        if unsigned.is_empty()
            || unsigned.iter().any(|byte| !byte.is_ascii_digit())
            || unsigned.len() > 1 && unsigned[0] == b'0'
            || negative && unsigned == b"0"
        {
            return Err(ParseError::InvalidInteger { position: start });
        }

        let text = std::str::from_utf8(digits)
            .map_err(|_| ParseError::InvalidInteger { position: start })?;
        let integer = text
            .parse::<i64>()
            .map_err(|_| ParseError::IntegerOverflow { position: start })?;
        Ok(Value::Integer(integer))
    }

    fn parse_bytes(&mut self) -> Result<&'a [u8], ParseError> {
        let start = self.position;
        let length_start = self.position;
        while self.peek()? != b':' {
            if !self.input[self.position].is_ascii_digit() {
                return Err(ParseError::InvalidStringLength { position: start });
            }
            self.position += 1;
        }
        let length_digits = &self.input[length_start..self.position];
        self.position += 1;

        if length_digits.is_empty()
            || length_digits.len() > 1 && length_digits.first() == Some(&b'0')
        {
            return Err(ParseError::InvalidStringLength { position: start });
        }
        let length = length_digits.iter().try_fold(0_usize, |value, byte| {
            value.checked_mul(10)?.checked_add(usize::from(byte - b'0'))
        });
        let length = length.ok_or(ParseError::InvalidStringLength { position: start })?;
        if length > self.limits.max_string_length {
            return Err(ParseError::StringTooLarge {
                position: start,
                length,
                maximum: self.limits.max_string_length,
            });
        }

        let end = self
            .position
            .checked_add(length)
            .ok_or(ParseError::InvalidStringLength { position: start })?;
        if end > self.input.len() {
            return Err(ParseError::UnexpectedEnd {
                position: self.input.len(),
            });
        }
        let bytes = &self.input[self.position..end];
        self.position = end;
        Ok(bytes)
    }

    fn parse_list(&mut self, depth: usize) -> Result<Value<'a>, ParseError> {
        let start = self.position;
        self.position += 1;
        let mut values = Vec::new();
        while self.peek()? != b'e' {
            if values.len() == self.limits.max_collection_entries {
                return Err(ParseError::CollectionTooLarge {
                    position: start,
                    maximum: self.limits.max_collection_entries,
                });
            }
            values.push(self.parse_value(depth + 1)?);
        }
        self.position += 1;
        Ok(Value::List(values))
    }

    fn parse_dictionary(&mut self, depth: usize) -> Result<Value<'a>, ParseError> {
        let start = self.position;
        self.position += 1;
        let mut entries = Vec::<DictionaryEntry<'a>>::new();
        while self.peek()? != b'e' {
            if entries.len() == self.limits.max_collection_entries {
                return Err(ParseError::CollectionTooLarge {
                    position: start,
                    maximum: self.limits.max_collection_entries,
                });
            }
            let key_position = self.position;
            if !self.peek()?.is_ascii_digit() {
                return Err(ParseError::InvalidToken {
                    position: key_position,
                });
            }
            self.consume_decoded_item()?;
            let key = self.parse_bytes()?;
            if self.allow_unsorted_dictionaries {
                if entries.iter().any(|previous| previous.key == key) {
                    return Err(ParseError::DictionaryKeysNotStrictlySorted {
                        position: key_position,
                    });
                }
            } else if entries.last().is_some_and(|previous| previous.key >= key) {
                return Err(ParseError::DictionaryKeysNotStrictlySorted {
                    position: key_position,
                });
            }
            let value = self.parse_value(depth + 1)?;
            entries.push(DictionaryEntry { key, value });
        }
        self.position += 1;
        if self.allow_unsorted_dictionaries {
            entries.sort_unstable_by(|left, right| left.key.cmp(right.key));
        }
        Ok(Value::Dictionary(entries))
    }

    fn consume_decoded_item(&mut self) -> Result<(), ParseError> {
        if self.decoded_items == self.limits.max_decoded_items {
            return Err(ParseError::TooManyDecodedItems {
                position: self.position,
                maximum: self.limits.max_decoded_items,
            });
        }
        self.decoded_items += 1;
        Ok(())
    }

    fn peek(&self) -> Result<u8, ParseError> {
        self.input
            .get(self.position)
            .copied()
            .ok_or(ParseError::UnexpectedEnd {
                position: self.position,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Limits, ParseError, Value, parse, parse_prefix, parse_prefix_with_limits,
        parse_with_limits, parse_with_limits_permissive_dictionaries,
    };

    #[test]
    fn parses_nested_canonical_value_with_spans() {
        let input = b"d1:ali1e3:twoe1:bi-2ee";
        let root = parse(input).expect("valid bencode");

        assert_eq!(root.span, 0..input.len());
        let Value::Dictionary(entries) = root.value else {
            panic!("expected dictionary");
        };
        assert_eq!(entries[0].key, b"a");
        assert_eq!(entries[0].value.span, 4..14);
        assert_eq!(entries[1].key, b"b");
        assert_eq!(entries[1].value.value, Value::Integer(-2));
    }

    #[test]
    fn rejects_truncated_and_structurally_invalid_values() {
        assert_eq!(
            parse(b"i12"),
            Err(ParseError::UnexpectedEnd { position: 3 })
        );
        assert_eq!(
            parse(b"d1:bi1e1:ai2ee"),
            Err(ParseError::DictionaryKeysNotStrictlySorted { position: 7 })
        );
        assert_eq!(
            parse(b"i03e"),
            Err(ParseError::InvalidInteger { position: 0 })
        );
        assert_eq!(
            parse(b"1:a1:b"),
            Err(ParseError::TrailingData { position: 3 })
        );
    }

    #[test]
    fn permissive_wire_mode_sorts_keys_but_rejects_duplicates() {
        let limits = Limits::default();
        let parsed = parse_with_limits_permissive_dictionaries(b"d1:bi1e1:ai2ee", limits)
            .expect("permissive dictionary order");
        let Value::Dictionary(entries) = parsed.value else {
            panic!("dictionary expected");
        };
        assert_eq!(entries[0].key, b"a");
        assert_eq!(entries[1].key, b"b");
        assert!(matches!(
            parse_with_limits_permissive_dictionaries(b"d1:ai1e1:ai2ee", limits),
            Err(ParseError::DictionaryKeysNotStrictlySorted { .. })
        ));
    }

    #[test]
    fn enforces_input_string_depth_and_collection_limits() {
        let limits = Limits {
            max_input_length: 16,
            max_string_length: 2,
            max_decoded_items: 8,
            max_depth: 1,
            max_collection_entries: 1,
        };
        assert!(matches!(
            parse_with_limits(b"17:abcdefghijklmnopq", limits),
            Err(ParseError::InputTooLarge { .. })
        ));
        assert!(matches!(
            parse_with_limits(b"3:abc", limits),
            Err(ParseError::StringTooLarge { .. })
        ));
        assert!(matches!(
            parse_with_limits(b"lli1eee", limits),
            Err(ParseError::NestingTooDeep { .. })
        ));
        assert!(matches!(
            parse_with_limits(b"li1ei2ee", limits),
            Err(ParseError::CollectionTooLarge { .. })
        ));
    }

    #[test]
    fn counts_values_and_dictionary_keys_before_insertion() {
        let exact = Limits {
            max_input_length: 64,
            max_string_length: 16,
            max_decoded_items: 4,
            max_depth: 4,
            max_collection_entries: 4,
        };
        parse_with_limits(b"d1:ali1eee", exact).expect("four retained items");

        let exceeded = Limits {
            max_decoded_items: 3,
            ..exact
        };
        assert_eq!(
            parse_with_limits(b"d1:ali1eee", exceeded),
            Err(ParseError::TooManyDecodedItems {
                position: 5,
                maximum: 3,
            })
        );
        assert_eq!(
            parse_with_limits_permissive_dictionaries(b"d1:ali1eee", exceeded),
            Err(ParseError::TooManyDecodedItems {
                position: 5,
                maximum: 3,
            })
        );
        assert_eq!(
            parse_prefix_with_limits(b"d1:ali1eeeDATA", exceeded),
            Err(ParseError::TooManyDecodedItems {
                position: 5,
                maximum: 3,
            })
        );
    }

    #[test]
    fn rejects_integer_overflow_and_noncanonical_lengths() {
        assert!(matches!(
            parse(b"i9223372036854775808e"),
            Err(ParseError::IntegerOverflow { .. })
        ));
        assert!(matches!(
            parse(b"03:abc"),
            Err(ParseError::InvalidStringLength { .. })
        ));
        assert!(matches!(
            parse(b"999999999999999999999999:"),
            Err(ParseError::InvalidStringLength { .. })
        ));
    }

    #[test]
    fn prefix_parse_returns_the_exact_consumed_length() {
        let (node, consumed) = parse_prefix(b"d1:ai1eeDATA").expect("dictionary prefix");

        assert_eq!(node.span, 0..8);
        assert_eq!(consumed, 8);
        assert_eq!(
            parse(b"d1:ai1eeDATA"),
            Err(ParseError::TrailingData { position: 8 })
        );
    }
}
