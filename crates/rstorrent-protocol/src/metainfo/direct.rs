use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;

use sha1::{Digest, Sha1};

use super::{
    MAX_METAINFO_PATH_COMPONENT_LENGTH, MAX_METAINFO_PATH_LENGTH, MAX_PIECE_LENGTH, Metainfo,
    MetainfoError, MetainfoFile, MetainfoLimits, MetainfoMode, MetainfoTracker,
    MetainfoTrackerTransport,
};
use crate::bencode::ParseError;

pub(super) struct ParsedOuter {
    pub metainfo: Metainfo,
    pub info_span: Range<usize>,
    pub trackers: Vec<MetainfoTracker>,
}

pub(super) fn parse_outer(
    bytes: &[u8],
    limits: MetainfoLimits,
) -> Result<ParsedOuter, MetainfoError> {
    let mut parser = Parser::new(bytes, limits.max_outer_bytes, limits)?;
    if parser.peek()? != b'd' {
        return Err(MetainfoError::RootIsNotDictionary);
    }
    parser.enter_container(b'd', 0)?;
    let mut previous_key = None;
    let mut parsed_info = None;
    let mut info_span = None;
    let mut announce = None;
    let mut trackers = Vec::new();
    let mut tracker_identities = HashSet::new();
    let mut entries = 0_usize;
    while parser.peek()? != b'e' {
        parser.check_collection(entries, parser.position)?;
        let key_position = parser.position;
        let key = parser.parse_bytes(1)?;
        check_dictionary_key(previous_key, key, key_position)?;
        previous_key = Some(key);
        entries += 1;
        match key {
            b"announce" if parser.peek()?.is_ascii_digit() => {
                announce = Some(parser.parse_bytes(1)?);
            }
            b"announce-list" => {
                parse_tracker_tiers(&mut parser, 1, &mut trackers, &mut tracker_identities)?
            }
            b"info" => {
                let start = parser.position;
                let metainfo = parse_info_dictionary(&mut parser, 1)?;
                info_span = Some(start..parser.position);
                parsed_info = Some(metainfo);
            }
            _ => parser.skip_value(1)?,
        }
    }
    parser.leave_container()?;
    parser.finish()?;

    let metainfo = parsed_info.ok_or(MetainfoError::MissingField("info"))?;
    let info_span = info_span.expect("parsed info has a span");
    enforce_info_length(info_span.len(), limits)?;
    if trackers.is_empty()
        && let Some(announce) = announce
        && let Some((url, transport)) = normalize_tracker_url(announce)
    {
        trackers.push(MetainfoTracker {
            tier: 0,
            position: 0,
            url,
            transport,
        });
    }
    Ok(ParsedOuter {
        metainfo,
        info_span,
        trackers,
    })
}

fn parse_tracker_tiers(
    parser: &mut Parser<'_>,
    depth: usize,
    trackers: &mut Vec<MetainfoTracker>,
    identities: &mut HashSet<Arc<str>>,
) -> Result<(), MetainfoError> {
    if parser.peek()? != b'l' {
        parser.skip_value(depth)?;
        return Ok(());
    }
    parser.enter_container(b'l', depth)?;
    let mut source_tiers = 0_usize;
    let mut retained_tier = 0_u32;
    while parser.peek()? != b'e' {
        parser.check_collection(source_tiers, parser.position)?;
        source_tiers += 1;
        if parser.peek()? != b'l' {
            parser.skip_value(depth + 1)?;
            continue;
        }
        parser.enter_container(b'l', depth + 1)?;
        let start = trackers.len();
        let mut source_entries = 0_usize;
        while parser.peek()? != b'e' {
            parser.check_collection(source_entries, parser.position)?;
            source_entries += 1;
            if parser.peek()?.is_ascii_digit() {
                let raw = parser.parse_bytes(depth + 2)?;
                if let Some((url, transport)) = normalize_tracker_url(raw)
                    && identities.insert(url.clone())
                {
                    trackers.push(MetainfoTracker {
                        tier: retained_tier,
                        position: u32::try_from(trackers.len() - start)
                            .expect("tracker count is below lexical-token bound"),
                        url,
                        transport,
                    });
                }
            } else {
                parser.skip_value(depth + 2)?;
            }
        }
        parser.leave_container()?;
        if trackers.len() != start {
            retained_tier = retained_tier
                .checked_add(1)
                .expect("tracker tiers are below lexical-token bound");
        }
    }
    parser.leave_container()?;
    Ok(())
}

fn normalize_tracker_url(bytes: &[u8]) -> Option<(Arc<str>, MetainfoTrackerTransport)> {
    let value = std::str::from_utf8(bytes).ok()?;
    if value.is_empty()
        || !value.is_ascii()
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
        || value.contains('#')
    {
        return None;
    }
    let (scheme, remainder) = value.split_once("://")?;
    let transport = if scheme.eq_ignore_ascii_case("udp") {
        MetainfoTrackerTransport::Udp
    } else if scheme.eq_ignore_ascii_case("http") {
        MetainfoTrackerTransport::Http
    } else if scheme.eq_ignore_ascii_case("https") {
        MetainfoTrackerTransport::Https
    } else {
        return None;
    };
    let authority_end = remainder.find(['/', '?']).unwrap_or(remainder.len());
    let authority = &remainder[..authority_end];
    if authority.is_empty() {
        return None;
    }
    if transport == MetainfoTrackerTransport::Udp
        && crate::magnet::UdpTrackerUrl::from_metainfo_url(value).is_none()
    {
        return None;
    }
    let (userinfo, host_port) = authority
        .rsplit_once('@')
        .map_or((None, authority), |(userinfo, host)| (Some(userinfo), host));
    if host_port.is_empty() {
        return None;
    }
    let mut normalized = String::with_capacity(value.len());
    normalized.push_str(match transport {
        MetainfoTrackerTransport::Udp => "udp://",
        MetainfoTrackerTransport::Http => "http://",
        MetainfoTrackerTransport::Https => "https://",
    });
    if let Some(userinfo) = userinfo {
        normalized.push_str(userinfo);
        normalized.push('@');
    }
    normalized.extend(
        host_port
            .chars()
            .map(|character| character.to_ascii_lowercase()),
    );
    normalized.push_str(&remainder[authority_end..]);
    Some((Arc::from(normalized), transport))
}

pub(super) fn parse_info(bytes: &[u8], limits: MetainfoLimits) -> Result<Metainfo, MetainfoError> {
    enforce_info_length(bytes.len(), limits)?;
    let mut parser = Parser::new(bytes, limits.max_info_bytes, limits)?;
    let metainfo = parse_info_dictionary(&mut parser, 0)?;
    parser.finish()?;
    Ok(metainfo)
}

fn parse_info_dictionary(parser: &mut Parser<'_>, depth: usize) -> Result<Metainfo, MetainfoError> {
    if parser.peek()? != b'd' {
        return Err(MetainfoError::InvalidField("info dictionary"));
    }
    let info_start = parser.position;
    parser.enter_container(b'd', depth)?;

    let mut previous_key = None;
    let mut entries = 0_usize;
    let mut length = None;
    let mut files = None;
    let mut name = None;
    let mut piece_length = None;
    let mut pieces = None;
    let mut private = false;
    let mut meta_version = false;

    while parser.peek()? != b'e' {
        parser.check_collection(entries, parser.position)?;
        let key_position = parser.position;
        let key = parser.parse_bytes(depth + 1)?;
        check_dictionary_key(previous_key, key, key_position)?;
        previous_key = Some(key);
        entries += 1;
        match key {
            b"files" => files = Some(parse_files(parser, depth + 1)?),
            b"length" => {
                length = Some(parse_nonnegative_integer(parser, depth + 1, "info.length")?)
            }
            b"meta version" => {
                parser.skip_value(depth + 1)?;
                meta_version = true;
            }
            b"name" => name = Some(parse_required_bytes(parser, depth + 1, "info.name")?),
            b"piece length" => {
                piece_length = Some(parse_positive_integer(
                    parser,
                    depth + 1,
                    "info.piece length",
                )?)
            }
            b"pieces" => pieces = Some(parse_required_bytes(parser, depth + 1, "info.pieces")?),
            b"private" => {
                if parser.peek()? != b'i' {
                    return Err(MetainfoError::InvalidField("info.private"));
                }
                private = match parser
                    .parse_integer(depth + 1)
                    .map_err(|_| MetainfoError::InvalidField("info.private"))?
                {
                    0 => false,
                    1 => true,
                    _ => return Err(MetainfoError::InvalidField("info.private")),
                }
            }
            _ => parser.skip_value(depth + 1)?,
        }
    }
    parser.leave_container()?;
    let info_end = parser.position;

    if meta_version {
        return Err(MetainfoError::Unsupported("v2 or hybrid info dictionary"));
    }

    let piece_length = piece_length.ok_or(MetainfoError::MissingField("info.piece length"))?;
    let piece_length = u32::try_from(piece_length)
        .map_err(|_| MetainfoError::InvalidField("info.piece length"))?;
    if piece_length > MAX_PIECE_LENGTH {
        return Err(MetainfoError::InvalidField("info.piece length"));
    }

    let name_bytes = name.ok_or(MetainfoError::MissingField("info.name"))?;
    let name = project_component(name_bytes);
    let (mode, mut files, total_length) = match (length, files) {
        (Some(length), None) => {
            if length == 0 {
                return Err(MetainfoError::InvalidField("info.length"));
            }
            (
                MetainfoMode::SingleFile,
                vec![MetainfoFile {
                    path: vec![name.clone()],
                    length,
                    offset: 0,
                    padding: false,
                }],
                length,
            )
        }
        (None, Some((files, total_length))) => (MetainfoMode::MultiFile, files, total_length),
        (Some(_), Some(_)) => {
            return Err(MetainfoError::InvalidField(
                "info must contain exactly one of length or files",
            ));
        }
        (None, None) => {
            return Err(MetainfoError::MissingField("info.length or info.files"));
        }
    };

    if total_length == 0 {
        return Err(MetainfoError::InvalidField("info total length"));
    }
    if mode == MetainfoMode::MultiFile {
        resolve_path_collisions(&mut files);
    }

    let expected_piece_count_u64 = total_length.div_ceil(u64::from(piece_length));
    let expected_piece_count =
        usize::try_from(expected_piece_count_u64).map_err(|_| MetainfoError::TooManyPieces {
            actual: usize::MAX,
            maximum: parser.limits.max_pieces,
        })?;
    if expected_piece_count > parser.limits.max_pieces {
        return Err(MetainfoError::TooManyPieces {
            actual: expected_piece_count,
            maximum: parser.limits.max_pieces,
        });
    }

    let pieces = pieces.ok_or(MetainfoError::MissingField("info.pieces"))?;
    if pieces.len() % 20 != 0 {
        return Err(MetainfoError::InvalidField(
            "info.pieces hash string length",
        ));
    }
    let actual_piece_count = pieces.len() / 20;
    if actual_piece_count > parser.limits.max_pieces {
        return Err(MetainfoError::TooManyPieces {
            actual: actual_piece_count,
            maximum: parser.limits.max_pieces,
        });
    }
    if actual_piece_count != expected_piece_count {
        return Err(MetainfoError::PieceCountMismatch {
            expected: expected_piece_count,
            actual: actual_piece_count,
        });
    }
    let piece_hashes = pieces
        .chunks_exact(20)
        .map(|hash| hash.try_into().expect("piece hash is exactly 20 bytes"))
        .collect();
    let info_hash = Sha1::digest(&parser.input[info_start..info_end]).into();

    Ok(Metainfo {
        info_hash,
        piece_hashes,
        piece_length,
        total_length,
        name,
        private,
        mode,
        files,
    })
}

fn parse_files(
    parser: &mut Parser<'_>,
    depth: usize,
) -> Result<(Vec<MetainfoFile>, u64), MetainfoError> {
    if parser.peek()? != b'l' {
        return Err(MetainfoError::InvalidField("info.files"));
    }
    parser.enter_container(b'l', depth)?;
    let mut files = Vec::new();
    let mut offset = 0_u64;
    while parser.peek()? != b'e' {
        if files.len() == parser.limits.max_files {
            return Err(MetainfoError::TooManyFiles {
                actual: files.len() + 1,
                maximum: parser.limits.max_files,
            });
        }
        parser.check_collection(files.len(), parser.position)?;
        let file = parse_file(parser, depth + 1, files.len(), offset)?;
        offset = offset
            .checked_add(file.length)
            .ok_or(MetainfoError::TotalLengthOverflow)?;
        files.push(file);
    }
    parser.leave_container()?;
    if files.is_empty() {
        return Err(MetainfoError::InvalidField("info.files"));
    }
    Ok((files, offset))
}

fn parse_file(
    parser: &mut Parser<'_>,
    depth: usize,
    index: usize,
    offset: u64,
) -> Result<MetainfoFile, MetainfoError> {
    if parser.peek()? != b'd' {
        return Err(MetainfoError::InvalidField("info.files entry"));
    }
    parser.enter_container(b'd', depth)?;
    let mut previous_key = None;
    let mut entries = 0_usize;
    let mut length = None;
    let mut path = None;
    let mut padding = false;
    let mut symlink = false;
    while parser.peek()? != b'e' {
        parser.check_collection(entries, parser.position)?;
        let key_position = parser.position;
        let key = parser.parse_bytes(depth + 1)?;
        check_dictionary_key(previous_key, key, key_position)?;
        previous_key = Some(key);
        entries += 1;
        match key {
            b"attr" => {
                let attributes = parse_required_bytes(parser, depth + 1, "info.files.attr")?;
                padding = attributes.contains(&b'p');
                symlink = attributes.contains(&b'l');
            }
            b"length" => {
                length = Some(parse_nonnegative_integer(
                    parser,
                    depth + 1,
                    "info.files.length",
                )?)
            }
            b"path" => path = Some(parse_path(parser, depth + 1, index)?),
            _ => parser.skip_value(depth + 1)?,
        }
    }
    parser.leave_container()?;
    if symlink {
        return Err(MetainfoError::Unsupported("BEP 47 symlink file"));
    }
    let length = length.ok_or(MetainfoError::MissingField("info.files.length"))?;
    let path = match path {
        Some(path) => path,
        None if padding => Vec::new(),
        None => return Err(MetainfoError::MissingField("info.files.path")),
    };
    if path.is_empty() && !padding {
        return Err(MetainfoError::UnsafePath {
            file: Some(index),
            reason: "path has no components",
        });
    }
    Ok(MetainfoFile {
        path,
        length,
        offset,
        padding,
    })
}

fn parse_path(
    parser: &mut Parser<'_>,
    depth: usize,
    file: usize,
) -> Result<Vec<String>, MetainfoError> {
    if parser.peek()? != b'l' {
        return Err(MetainfoError::InvalidField("info.files.path"));
    }
    parser.enter_container(b'l', depth)?;
    let mut source_hash = Sha1::new();
    let mut components = Vec::new();
    let mut last_component = None;
    let mut encoded_length = 0_usize;
    let mut collapsed = false;
    let mut count = 0_usize;
    while parser.peek()? != b'e' {
        if count == parser.limits.max_path_components {
            return Err(MetainfoError::UnsafePath {
                file: Some(file),
                reason: "path has too many components",
            });
        }
        parser.check_collection(count, parser.position)?;
        let raw = parse_required_bytes(parser, depth + 1, "info.files.path component")?;
        if raw.len() > parser.limits.max_path_component_bytes {
            return Err(MetainfoError::UnsafePath {
                file: Some(file),
                reason: "component is too long",
            });
        }
        source_hash.update((raw.len() as u64).to_be_bytes());
        source_hash.update(raw);
        let component = project_component(raw);
        let next_length = encoded_length
            .checked_add(component.len() + usize::from(count != 0))
            .unwrap_or(usize::MAX);
        if next_length > parser.limits.max_path_bytes {
            return Err(MetainfoError::UnsafePath {
                file: Some(file),
                reason: "path is too long",
            });
        }
        if !collapsed && next_length <= MAX_METAINFO_PATH_LENGTH {
            encoded_length = next_length;
            components.push(component.clone());
        } else {
            collapsed = true;
            if components.len() > 8 {
                components.truncate(8);
            }
        }
        last_component = Some(component);
        count += 1;
    }
    parser.leave_container()?;
    if count == 0 {
        return Err(MetainfoError::UnsafePath {
            file: Some(file),
            reason: "path has no components",
        });
    }
    if collapsed {
        components.truncate(8);
        components.push(format!(
            "path~{}",
            hex_digest(source_hash.finalize().as_slice())
        ));
        if let Some(last) = last_component
            && components.last() != Some(&last)
        {
            components.push(last);
        }
    }
    debug_assert!(joined_path_length(&components) <= MAX_METAINFO_PATH_LENGTH);
    Ok(components)
}

fn parse_required_bytes<'a>(
    parser: &mut Parser<'a>,
    depth: usize,
    field: &'static str,
) -> Result<&'a [u8], MetainfoError> {
    if !parser.peek()?.is_ascii_digit() {
        return Err(MetainfoError::InvalidField(field));
    }
    parser.parse_bytes(depth).map_err(Into::into)
}

fn parse_nonnegative_integer(
    parser: &mut Parser<'_>,
    depth: usize,
    field: &'static str,
) -> Result<u64, MetainfoError> {
    let value = parser
        .parse_integer(depth)
        .map_err(|_| MetainfoError::InvalidField(field))?;
    u64::try_from(value).map_err(|_| MetainfoError::InvalidField(field))
}

fn parse_positive_integer(
    parser: &mut Parser<'_>,
    depth: usize,
    field: &'static str,
) -> Result<u64, MetainfoError> {
    let value = parse_nonnegative_integer(parser, depth, field)?;
    if value == 0 {
        return Err(MetainfoError::InvalidField(field));
    }
    Ok(value)
}

fn enforce_info_length(length: usize, limits: MetainfoLimits) -> Result<(), MetainfoError> {
    if length > limits.max_info_bytes {
        return Err(MetainfoError::InfoTooLarge {
            length,
            maximum: limits.max_info_bytes,
        });
    }
    Ok(())
}

fn check_dictionary_key(
    previous: Option<&[u8]>,
    current: &[u8],
    position: usize,
) -> Result<(), MetainfoError> {
    if previous.is_some_and(|previous| previous >= current) {
        return Err(ParseError::DictionaryKeysNotStrictlySorted { position }.into());
    }
    Ok(())
}

struct Parser<'a> {
    input: &'a [u8],
    position: usize,
    tokens: usize,
    limits: MetainfoLimits,
}

impl<'a> Parser<'a> {
    fn new(input: &'a [u8], maximum: usize, limits: MetainfoLimits) -> Result<Self, ParseError> {
        if input.len() > maximum {
            return Err(ParseError::InputTooLarge {
                length: input.len(),
                maximum,
            });
        }
        Ok(Self {
            input,
            position: 0,
            tokens: 0,
            limits,
        })
    }

    fn finish(&self) -> Result<(), ParseError> {
        if self.position != self.input.len() {
            return Err(ParseError::TrailingData {
                position: self.position,
            });
        }
        Ok(())
    }

    fn skip_value(&mut self, depth: usize) -> Result<(), ParseError> {
        self.check_depth(depth)?;
        match self.peek()? {
            b'i' => {
                self.parse_integer(depth)?;
            }
            b'0'..=b'9' => {
                self.parse_bytes(depth)?;
            }
            token @ (b'l' | b'd') => {
                self.enter_container(token, depth)?;
                let mut previous_key = None;
                let mut entries = 0_usize;
                while self.peek()? != b'e' {
                    self.check_collection(entries, self.position)?;
                    if token == b'd' {
                        let key_position = self.position;
                        let key = self.parse_bytes(depth + 1)?;
                        if previous_key.is_some_and(|previous| previous >= key) {
                            return Err(ParseError::DictionaryKeysNotStrictlySorted {
                                position: key_position,
                            });
                        }
                        previous_key = Some(key);
                    }
                    self.skip_value(depth + 1)?;
                    entries += 1;
                }
                self.leave_container()?;
            }
            _ => {
                return Err(ParseError::InvalidToken {
                    position: self.position,
                });
            }
        }
        Ok(())
    }

    fn parse_integer(&mut self, depth: usize) -> Result<i64, ParseError> {
        self.check_depth(depth)?;
        let start = self.position;
        if self.peek()? != b'i' {
            return Err(ParseError::InvalidToken { position: start });
        }
        self.consume_token()?;
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
        text.parse()
            .map_err(|_| ParseError::IntegerOverflow { position: start })
    }

    fn parse_bytes(&mut self, depth: usize) -> Result<&'a [u8], ParseError> {
        self.check_depth(depth)?;
        let start = self.position;
        if !self.peek()?.is_ascii_digit() {
            return Err(ParseError::InvalidToken { position: start });
        }
        self.consume_token()?;
        let length_start = self.position;
        while self.peek()? != b':' {
            if !self.input[self.position].is_ascii_digit() {
                return Err(ParseError::InvalidStringLength { position: start });
            }
            self.position += 1;
        }
        let digits = &self.input[length_start..self.position];
        self.position += 1;
        if digits.is_empty() || digits.len() > 1 && digits[0] == b'0' {
            return Err(ParseError::InvalidStringLength { position: start });
        }
        let length = digits.iter().try_fold(0_usize, |value, byte| {
            value.checked_mul(10)?.checked_add(usize::from(byte - b'0'))
        });
        let length = length.ok_or(ParseError::InvalidStringLength { position: start })?;
        if length > self.limits.max_string_bytes {
            return Err(ParseError::StringTooLarge {
                position: start,
                length,
                maximum: self.limits.max_string_bytes,
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
        let value = &self.input[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn enter_container(&mut self, token: u8, depth: usize) -> Result<(), ParseError> {
        self.check_depth(depth)?;
        if self.peek()? != token {
            return Err(ParseError::InvalidToken {
                position: self.position,
            });
        }
        self.consume_token()?;
        self.position += 1;
        Ok(())
    }

    fn leave_container(&mut self) -> Result<(), ParseError> {
        if self.peek()? != b'e' {
            return Err(ParseError::InvalidToken {
                position: self.position,
            });
        }
        self.consume_token()?;
        self.position += 1;
        Ok(())
    }

    fn check_depth(&self, depth: usize) -> Result<(), ParseError> {
        if depth >= self.limits.max_depth {
            return Err(ParseError::NestingTooDeep {
                position: self.position,
                maximum: self.limits.max_depth,
            });
        }
        Ok(())
    }

    fn check_collection(&self, entries: usize, position: usize) -> Result<(), ParseError> {
        if entries == self.limits.max_collection_entries {
            return Err(ParseError::CollectionTooLarge {
                position,
                maximum: self.limits.max_collection_entries,
            });
        }
        Ok(())
    }

    fn consume_token(&mut self) -> Result<(), ParseError> {
        if self.tokens == self.limits.max_decoded_items {
            return Err(ParseError::TooManyDecodedItems {
                position: self.position,
                maximum: self.limits.max_decoded_items,
            });
        }
        self.tokens += 1;
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

fn project_component(bytes: &[u8]) -> String {
    let digest = Sha1::digest(bytes);
    let mut needs_suffix = false;
    let mut projected = match std::str::from_utf8(bytes) {
        Ok(text) => {
            let mut projected = String::with_capacity(text.len());
            for character in text.chars() {
                if character.is_control()
                    || matches!(
                        character,
                        '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                    )
                {
                    projected.push('_');
                    needs_suffix = true;
                } else {
                    if !character.is_ascii() {
                        needs_suffix = true;
                    }
                    projected.push(character);
                }
            }
            while projected.ends_with([' ', '.']) {
                projected.pop();
                needs_suffix = true;
            }
            projected
        }
        Err(_) => {
            needs_suffix = true;
            "_".to_owned()
        }
    };

    if projected.is_empty()
        || matches!(projected.as_str(), "." | "..")
        || is_windows_reserved(&projected)
    {
        projected = "_".to_owned();
        needs_suffix = true;
    }
    if projected.len() > MAX_METAINFO_PATH_COMPONENT_LENGTH {
        needs_suffix = true;
    }
    if needs_suffix {
        append_suffix(
            &mut projected,
            &format!("~{}", hex_digest(digest.as_slice())),
        );
    } else {
        truncate_utf8(&mut projected, MAX_METAINFO_PATH_COMPONENT_LENGTH);
    }
    projected
}

fn resolve_path_collisions(files: &mut [MetainfoFile]) {
    let mut occupied_files = HashSet::with_capacity(files.len());
    let mut occupied_directories = HashSet::new();
    for (index, file) in files.iter_mut().enumerate() {
        if file.path.is_empty() {
            continue;
        }
        while path_conflicts(&file.path, &occupied_files, &occupied_directories) {
            let mut hash = Sha1::new();
            for component in &file.path {
                hash.update((component.len() as u64).to_be_bytes());
                hash.update(component.as_bytes());
            }
            hash.update((index as u64).to_be_bytes());
            let suffix = format!("~{}", hex_digest(hash.finalize().as_slice()));
            append_suffix(&mut file.path[0], &suffix);
        }
        let normalized: Vec<String> = file.path.iter().map(|value| value.to_lowercase()).collect();
        occupied_files.insert(normalized.join("/"));
        for end in 1..normalized.len() {
            occupied_directories.insert(normalized[..end].join("/"));
        }
    }
}

fn path_conflicts(
    path: &[String],
    occupied_files: &HashSet<String>,
    occupied_directories: &HashSet<String>,
) -> bool {
    let normalized: Vec<String> = path.iter().map(|value| value.to_lowercase()).collect();
    let full = normalized.join("/");
    occupied_files.contains(&full)
        || occupied_directories.contains(&full)
        || (1..normalized.len()).any(|end| occupied_files.contains(&normalized[..end].join("/")))
}

fn append_suffix(value: &mut String, suffix: &str) {
    let prefix_limit = MAX_METAINFO_PATH_COMPONENT_LENGTH.saturating_sub(suffix.len());
    truncate_utf8(value, prefix_limit);
    value.push_str(suffix);
}

fn truncate_utf8(value: &mut String, maximum: usize) {
    if value.len() <= maximum {
        return;
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}

fn joined_path_length(path: &[String]) -> usize {
    path.iter().map(String::len).sum::<usize>() + path.len().saturating_sub(1)
}

fn is_windows_reserved(value: &str) -> bool {
    let stem = value
        .split('.')
        .next()
        .unwrap_or(value)
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
