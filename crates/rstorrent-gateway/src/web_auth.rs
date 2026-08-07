use std::error::Error;
use std::fmt;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};

pub const INITIAL_WINDOW_SECONDS: i64 = 10 * 60;
pub const PAIRING_TICKET_SECONDS: i64 = 10 * 60;
pub const SESSION_IDLE_SECONDS: i64 = 180 * 24 * 60 * 60;
pub const SESSION_TOUCH_SECONDS: i64 = 60 * 60;
pub const MAX_WEB_SESSIONS: usize = 32;
pub const MAX_SESSION_LABEL_BYTES: usize = 80;
pub const MAX_PAIRING_FAILURES: u8 = 5;

const SESSION_TOKEN_BYTES: usize = 32;
const SESSION_ID_BYTES: usize = 16;
const TICKET_SALT_BYTES: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebAccessPolicy {
    Unconfigured,
    LocalOpen,
    Paired,
}

impl WebAccessPolicy {
    fn stored(self) -> &'static str {
        match self {
            Self::Unconfigured => "unconfigured",
            Self::LocalOpen => "local_open",
            Self::Paired => "paired",
        }
    }

    fn parse(value: &str) -> Result<Self, WebAuthError> {
        match value {
            "unconfigured" => Ok(Self::Unconfigured),
            "local_open" => Ok(Self::LocalOpen),
            "paired" => Ok(Self::Paired),
            value => Err(WebAuthError::Corrupt(format!(
                "unknown web access policy {value:?}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IssuedWebSession {
    pub id: String,
    pub token: String,
    pub label: String,
    pub created_at: i64,
    pub last_used_at: i64,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedWebSession {
    pub id: String,
    pub label: String,
    pub created_at: i64,
    pub last_used_at: i64,
    pub expires_at: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairingTicket {
    pub code: String,
    pub expires_at: i64,
}

#[derive(Debug)]
pub enum WebAuthError {
    Storage(rusqlite::Error),
    Random(getrandom::Error),
    InvalidLabel,
    InvalidCode,
    TicketExpired,
    TicketAttemptsExhausted,
    NoPairingTicket,
    InvalidSession,
    SessionExpired,
    SessionLimit,
    PolicyAlreadyConfigured,
    Corrupt(String),
}

impl fmt::Display for WebAuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => write!(formatter, "web auth storage: {error}"),
            Self::Random(error) => write!(formatter, "web auth randomness: {error}"),
            Self::InvalidLabel => formatter.write_str("browser label is invalid"),
            Self::InvalidCode => formatter.write_str("pairing code was rejected"),
            Self::TicketExpired => formatter.write_str("pairing code expired"),
            Self::TicketAttemptsExhausted => {
                formatter.write_str("pairing code attempt limit reached")
            }
            Self::NoPairingTicket => formatter.write_str("no pairing code is active"),
            Self::InvalidSession => formatter.write_str("browser session was rejected"),
            Self::SessionExpired => formatter.write_str("browser session expired"),
            Self::SessionLimit => formatter.write_str("browser session limit reached"),
            Self::PolicyAlreadyConfigured => {
                formatter.write_str("web access policy is already configured")
            }
            Self::Corrupt(message) => write!(formatter, "web auth state is corrupt: {message}"),
        }
    }
}

impl Error for WebAuthError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Storage(error) => Some(error),
            Self::Random(error) => Some(error),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for WebAuthError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error)
    }
}

impl From<getrandom::Error> for WebAuthError {
    fn from(error: getrandom::Error) -> Self {
        Self::Random(error)
    }
}

pub struct WebAuthStore {
    connection: Connection,
}

impl fmt::Debug for WebAuthStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebAuthStore")
            .finish_non_exhaustive()
    }
}

impl WebAuthStore {
    pub fn open(path: &Path) -> Result<Self, WebAuthError> {
        let connection = Connection::open(path)?;
        Self::initialize(connection)
    }

    pub fn in_memory() -> Result<Self, WebAuthError> {
        Self::initialize(Connection::open_in_memory()?)
    }

    fn initialize(connection: Connection) -> Result<Self, WebAuthError> {
        connection.busy_timeout(std::time::Duration::from_secs(1))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE IF NOT EXISTS web_auth_state (
                 singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                 policy TEXT NOT NULL
             );
             INSERT OR IGNORE INTO web_auth_state(singleton, policy)
             VALUES (1, 'unconfigured');
             CREATE TABLE IF NOT EXISTS web_auth_sessions (
                 session_id TEXT PRIMARY KEY,
                 token_digest BLOB NOT NULL UNIQUE CHECK(length(token_digest) = 32),
                 label TEXT NOT NULL,
                 created_at INTEGER NOT NULL,
                 last_used_at INTEGER NOT NULL,
                 expires_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS web_auth_pairing_ticket (
                 singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                 code_digest BLOB NOT NULL CHECK(length(code_digest) = 32),
                 salt BLOB NOT NULL CHECK(length(salt) = 16),
                 created_at INTEGER NOT NULL,
                 expires_at INTEGER NOT NULL,
                 failures INTEGER NOT NULL CHECK(failures >= 0 AND failures <= 5)
             );",
        )?;
        Ok(Self { connection })
    }

    pub fn policy(&self) -> Result<WebAccessPolicy, WebAuthError> {
        policy_from_connection(&self.connection)
    }

    pub fn commit_initial_local_open(&mut self) -> Result<(), WebAuthError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_unconfigured(&transaction)?;
        set_policy(&transaction, WebAccessPolicy::LocalOpen)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn commit_initial_paired(
        &mut self,
        label: &str,
        now: i64,
    ) -> Result<IssuedWebSession, WebAuthError> {
        validate_label(label)?;
        let material = SessionMaterial::generate()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        require_unconfigured(&transaction)?;
        let session = insert_session(&transaction, label, now, material)?;
        set_policy(&transaction, WebAccessPolicy::Paired)?;
        transaction.commit()?;
        Ok(session)
    }

    pub fn enable_paired(
        &mut self,
        label: &str,
        now: i64,
    ) -> Result<IssuedWebSession, WebAuthError> {
        validate_label(label)?;
        let material = SessionMaterial::generate()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let session = insert_session(&transaction, label, now, material)?;
        set_policy(&transaction, WebAccessPolicy::Paired)?;
        transaction.commit()?;
        Ok(session)
    }

    pub fn enable_local_open(&mut self) -> Result<(), WebAuthError> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        set_policy(&transaction, WebAccessPolicy::LocalOpen)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn issue_session(
        &mut self,
        label: &str,
        now: i64,
    ) -> Result<IssuedWebSession, WebAuthError> {
        validate_label(label)?;
        let material = SessionMaterial::generate()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let session = insert_session(&transaction, label, now, material)?;
        transaction.commit()?;
        Ok(session)
    }

    pub fn authenticate(
        &mut self,
        token: &str,
        now: i64,
    ) -> Result<AuthorizedWebSession, WebAuthError> {
        let digest = session_token_digest(token)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let session = select_session(&transaction, &digest)?.ok_or(WebAuthError::InvalidSession)?;
        if now >= session.expires_at {
            transaction.execute(
                "DELETE FROM web_auth_sessions WHERE session_id = ?1",
                params![session.id],
            )?;
            transaction.commit()?;
            return Err(WebAuthError::SessionExpired);
        }
        let session = if now.saturating_sub(session.last_used_at) >= SESSION_TOUCH_SECONDS {
            let expires_at = now.saturating_add(SESSION_IDLE_SECONDS);
            transaction.execute(
                "UPDATE web_auth_sessions
                 SET last_used_at = ?2, expires_at = ?3
                 WHERE session_id = ?1",
                params![session.id, now, expires_at],
            )?;
            AuthorizedWebSession {
                last_used_at: now,
                expires_at,
                ..session
            }
        } else {
            session
        };
        transaction.commit()?;
        Ok(session)
    }

    pub fn create_pairing_ticket(&mut self, now: i64) -> Result<PairingTicket, WebAuthError> {
        let code = generate_pairing_code()?;
        let mut salt = [0_u8; TICKET_SALT_BYTES];
        getrandom::fill(&mut salt)?;
        let digest = pairing_code_digest(&code, &salt);
        let expires_at = now.saturating_add(PAIRING_TICKET_SECONDS);
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("DELETE FROM web_auth_pairing_ticket", [])?;
        transaction.execute(
            "INSERT INTO web_auth_pairing_ticket(
                singleton, code_digest, salt, created_at, expires_at, failures
             ) VALUES (1, ?1, ?2, ?3, ?4, 0)",
            params![digest.as_slice(), salt.as_slice(), now, expires_at],
        )?;
        transaction.commit()?;
        Ok(PairingTicket { code, expires_at })
    }

    pub fn redeem_pairing_ticket(
        &mut self,
        code: &str,
        label: &str,
        now: i64,
    ) -> Result<IssuedWebSession, WebAuthError> {
        validate_label(label)?;
        if !valid_pairing_code(code) {
            return Err(WebAuthError::InvalidCode);
        }
        let material = SessionMaterial::generate()?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ticket = select_pairing_ticket(&transaction)?.ok_or(WebAuthError::NoPairingTicket)?;
        if now >= ticket.expires_at {
            transaction.execute("DELETE FROM web_auth_pairing_ticket", [])?;
            transaction.commit()?;
            return Err(WebAuthError::TicketExpired);
        }
        let candidate = pairing_code_digest(code, &ticket.salt);
        if !constant_time_equal(&candidate, &ticket.digest) {
            let failures = ticket.failures.saturating_add(1);
            if failures >= MAX_PAIRING_FAILURES {
                transaction.execute("DELETE FROM web_auth_pairing_ticket", [])?;
                transaction.commit()?;
                return Err(WebAuthError::TicketAttemptsExhausted);
            }
            transaction.execute(
                "UPDATE web_auth_pairing_ticket SET failures = ?1 WHERE singleton = 1",
                params![failures],
            )?;
            transaction.commit()?;
            return Err(WebAuthError::InvalidCode);
        }
        let session = insert_session(&transaction, label, now, material)?;
        transaction.execute("DELETE FROM web_auth_pairing_ticket", [])?;
        transaction.commit()?;
        Ok(session)
    }

    pub fn list_sessions(&self, now: i64) -> Result<Vec<AuthorizedWebSession>, WebAuthError> {
        let mut statement = self.connection.prepare(
            "SELECT session_id, label, created_at, last_used_at, expires_at
             FROM web_auth_sessions
             WHERE expires_at > ?1
             ORDER BY created_at, session_id",
        )?;
        let rows = statement.query_map(params![now], |row| {
            Ok(AuthorizedWebSession {
                id: row.get(0)?,
                label: row.get(1)?,
                created_at: row.get(2)?,
                last_used_at: row.get(3)?,
                expires_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn revoke_session(&mut self, session_id: &str) -> Result<bool, WebAuthError> {
        Ok(self.connection.execute(
            "DELETE FROM web_auth_sessions WHERE session_id = ?1",
            params![session_id],
        )? == 1)
    }

    pub fn revoke_all_other_sessions(
        &mut self,
        current_session_id: &str,
    ) -> Result<usize, WebAuthError> {
        self.connection
            .execute(
                "DELETE FROM web_auth_sessions WHERE session_id <> ?1",
                params![current_session_id],
            )
            .map_err(Into::into)
    }

    pub fn reap_expired(&mut self, now: i64) -> Result<usize, WebAuthError> {
        let sessions = self.connection.execute(
            "DELETE FROM web_auth_sessions WHERE expires_at <= ?1",
            params![now],
        )?;
        let tickets = self.connection.execute(
            "DELETE FROM web_auth_pairing_ticket WHERE expires_at <= ?1",
            params![now],
        )?;
        Ok(sessions.saturating_add(tickets))
    }
}

struct SessionMaterial {
    id: String,
    token: String,
    digest: [u8; 32],
}

impl SessionMaterial {
    fn generate() -> Result<Self, WebAuthError> {
        let mut id = [0_u8; SESSION_ID_BYTES];
        let mut token = [0_u8; SESSION_TOKEN_BYTES];
        getrandom::fill(&mut id)?;
        getrandom::fill(&mut token)?;
        let token = URL_SAFE_NO_PAD.encode(token);
        Ok(Self {
            id: hex(&id),
            digest: session_token_digest(&token)?,
            token,
        })
    }
}

struct StoredPairingTicket {
    digest: Vec<u8>,
    salt: Vec<u8>,
    expires_at: i64,
    failures: u8,
}

fn policy_from_connection(connection: &Connection) -> Result<WebAccessPolicy, WebAuthError> {
    let value = connection.query_row(
        "SELECT policy FROM web_auth_state WHERE singleton = 1",
        [],
        |row| row.get::<_, String>(0),
    )?;
    WebAccessPolicy::parse(&value)
}

fn require_unconfigured(transaction: &Transaction<'_>) -> Result<(), WebAuthError> {
    if policy_from_connection(transaction)? != WebAccessPolicy::Unconfigured {
        return Err(WebAuthError::PolicyAlreadyConfigured);
    }
    Ok(())
}

fn set_policy(transaction: &Transaction<'_>, policy: WebAccessPolicy) -> Result<(), WebAuthError> {
    transaction.execute(
        "UPDATE web_auth_state SET policy = ?1 WHERE singleton = 1",
        params![policy.stored()],
    )?;
    Ok(())
}

fn insert_session(
    transaction: &Transaction<'_>,
    label: &str,
    now: i64,
    material: SessionMaterial,
) -> Result<IssuedWebSession, WebAuthError> {
    let count = transaction.query_row(
        "SELECT COUNT(*) FROM web_auth_sessions WHERE expires_at > ?1",
        params![now],
        |row| row.get::<_, i64>(0),
    )?;
    if count >= MAX_WEB_SESSIONS as i64 {
        return Err(WebAuthError::SessionLimit);
    }
    transaction.execute(
        "DELETE FROM web_auth_sessions WHERE expires_at <= ?1",
        params![now],
    )?;
    let expires_at = now.saturating_add(SESSION_IDLE_SECONDS);
    transaction.execute(
        "INSERT INTO web_auth_sessions(
            session_id, token_digest, label, created_at, last_used_at, expires_at
         ) VALUES (?1, ?2, ?3, ?4, ?4, ?5)",
        params![
            material.id,
            material.digest.as_slice(),
            label,
            now,
            expires_at
        ],
    )?;
    Ok(IssuedWebSession {
        id: material.id,
        token: material.token,
        label: label.to_owned(),
        created_at: now,
        last_used_at: now,
        expires_at,
    })
}

fn select_session(
    transaction: &Transaction<'_>,
    digest: &[u8; 32],
) -> Result<Option<AuthorizedWebSession>, WebAuthError> {
    transaction
        .query_row(
            "SELECT session_id, label, created_at, last_used_at, expires_at
             FROM web_auth_sessions WHERE token_digest = ?1",
            params![digest.as_slice()],
            |row| {
                Ok(AuthorizedWebSession {
                    id: row.get(0)?,
                    label: row.get(1)?,
                    created_at: row.get(2)?,
                    last_used_at: row.get(3)?,
                    expires_at: row.get(4)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn select_pairing_ticket(
    transaction: &Transaction<'_>,
) -> Result<Option<StoredPairingTicket>, WebAuthError> {
    transaction
        .query_row(
            "SELECT code_digest, salt, expires_at, failures
             FROM web_auth_pairing_ticket WHERE singleton = 1",
            [],
            |row| {
                Ok(StoredPairingTicket {
                    digest: row.get(0)?,
                    salt: row.get(1)?,
                    expires_at: row.get(2)?,
                    failures: row.get(3)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn validate_label(label: &str) -> Result<(), WebAuthError> {
    if label.is_empty()
        || label.len() > MAX_SESSION_LABEL_BYTES
        || label.chars().any(char::is_control)
    {
        return Err(WebAuthError::InvalidLabel);
    }
    Ok(())
}

fn valid_pairing_code(code: &str) -> bool {
    code.len() == 4 && code.bytes().all(|byte| byte.is_ascii_digit())
}

fn generate_pairing_code() -> Result<String, WebAuthError> {
    loop {
        let mut bytes = [0_u8; 2];
        getrandom::fill(&mut bytes)?;
        let value = u16::from_le_bytes(bytes);
        if value < 60_000 {
            return Ok(format!("{:04}", value % 10_000));
        }
    }
}

fn session_token_digest(token: &str) -> Result<[u8; 32], WebAuthError> {
    let decoded = URL_SAFE_NO_PAD
        .decode(token)
        .map_err(|_| WebAuthError::InvalidSession)?;
    if decoded.len() != SESSION_TOKEN_BYTES {
        return Err(WebAuthError::InvalidSession);
    }
    let mut hasher = Sha256::new();
    hasher.update(b"rstorrent-web-session-v1\0");
    hasher.update(decoded);
    Ok(hasher.finalize().into())
}

fn pairing_code_digest(code: &str, salt: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"rstorrent-web-pairing-v1\0");
    hasher.update(salt);
    hasher.update(code.as_bytes());
    hasher.finalize().into()
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(std::path::PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let mut random = [0_u8; 8];
            getrandom::fill(&mut random).expect("random test directory");
            let path = std::env::temp_dir().join(format!(
                "rstorrent-web-auth-{}-{}",
                std::process::id(),
                hex(&random)
            ));
            std::fs::create_dir(&path).expect("create test directory");
            Self(path)
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).expect("remove test directory");
        }
    }

    #[test]
    fn fresh_policy_is_committed_once() {
        let mut store = WebAuthStore::in_memory().expect("store");
        assert_eq!(
            store.policy().expect("policy"),
            WebAccessPolicy::Unconfigured
        );
        store.commit_initial_local_open().expect("commit");
        assert_eq!(store.policy().expect("policy"), WebAccessPolicy::LocalOpen);
        assert!(matches!(
            store.commit_initial_local_open(),
            Err(WebAuthError::PolicyAlreadyConfigured)
        ));
    }

    #[test]
    fn paired_policy_issues_persistent_opaque_session() {
        let mut store = WebAuthStore::in_memory().expect("store");
        let issued = store
            .commit_initial_paired("Firefox on laptop", 100)
            .expect("paired");
        assert_eq!(store.policy().expect("policy"), WebAccessPolicy::Paired);
        assert_eq!(issued.token.len(), 43);
        let authorized = store.authenticate(&issued.token, 101).expect("auth");
        assert_eq!(authorized.id, issued.id);
        assert_eq!(authorized.label, "Firefox on laptop");
        assert!(!format!("{store:?}").contains(&issued.token));
    }

    #[test]
    fn policy_and_session_survive_store_reopen() {
        let directory = TestDirectory::new();
        let path = directory.0.join("web-auth.sqlite3");
        let issued = {
            let mut store = WebAuthStore::open(&path).expect("open");
            store
                .commit_initial_paired("Persistent browser", 500)
                .expect("paired")
        };
        let mut reopened = WebAuthStore::open(&path).expect("reopen");
        assert_eq!(reopened.policy().expect("policy"), WebAccessPolicy::Paired);
        assert_eq!(
            reopened
                .authenticate(&issued.token, 501)
                .expect("authenticate")
                .label,
            "Persistent browser"
        );
    }

    #[test]
    fn pairing_ticket_is_four_digits_single_use_and_attempt_bounded() {
        let mut store = WebAuthStore::in_memory().expect("store");
        store.commit_initial_local_open().expect("open");
        let ticket = store.create_pairing_ticket(1_000).expect("ticket");
        assert!(valid_pairing_code(&ticket.code));
        for _ in 0..4 {
            assert!(matches!(
                store.redeem_pairing_ticket("999x", "Other browser", 1_001),
                Err(WebAuthError::InvalidCode)
            ));
        }
        let session = store
            .redeem_pairing_ticket(&ticket.code, "Other browser", 1_002)
            .expect("redeem");
        assert!(store.authenticate(&session.token, 1_003).is_ok());
        assert!(matches!(
            store.redeem_pairing_ticket(&ticket.code, "Replay", 1_004),
            Err(WebAuthError::NoPairingTicket)
        ));

        let ticket = store.create_pairing_ticket(2_000).expect("ticket");
        let wrong = if ticket.code == "0000" {
            "0001"
        } else {
            "0000"
        };
        for attempt in 1..=MAX_PAIRING_FAILURES {
            let error = store
                .redeem_pairing_ticket(wrong, "Other browser", 2_001)
                .expect_err("wrong code");
            if attempt == MAX_PAIRING_FAILURES {
                assert!(matches!(error, WebAuthError::TicketAttemptsExhausted));
            } else {
                assert!(matches!(error, WebAuthError::InvalidCode));
            }
        }
        assert!(matches!(
            store.redeem_pairing_ticket(&ticket.code, "Too late", 2_002),
            Err(WebAuthError::NoPairingTicket)
        ));
    }

    #[test]
    fn expiry_touch_listing_and_revocation_are_bounded() {
        let mut store = WebAuthStore::in_memory().expect("store");
        let first = store.issue_session("First", 10).expect("first");
        let second = store.issue_session("Second", 11).expect("second");
        let touched = store
            .authenticate(&first.token, 10 + SESSION_TOUCH_SECONDS)
            .expect("touch");
        assert_eq!(touched.last_used_at, 10 + SESSION_TOUCH_SECONDS);
        assert_eq!(store.list_sessions(20).expect("list").len(), 2);
        assert!(store.revoke_session(&second.id).expect("revoke"));
        assert!(matches!(
            store.authenticate(&second.token, 21),
            Err(WebAuthError::InvalidSession)
        ));
        assert!(matches!(
            store.authenticate(&first.token, touched.expires_at),
            Err(WebAuthError::SessionExpired)
        ));
    }

    #[test]
    fn session_limit_fails_without_eviction() {
        let mut store = WebAuthStore::in_memory().expect("store");
        let mut first = None;
        for index in 0..MAX_WEB_SESSIONS {
            let issued = store
                .issue_session(&format!("Browser {index}"), 100)
                .expect("session");
            first.get_or_insert(issued);
        }
        assert!(matches!(
            store.issue_session("One too many", 100),
            Err(WebAuthError::SessionLimit)
        ));
        assert!(
            store
                .authenticate(&first.expect("first").token, 101)
                .is_ok()
        );
    }

    #[test]
    fn invalid_labels_and_expired_ticket_fail_cleanly() {
        let mut store = WebAuthStore::in_memory().expect("store");
        assert!(matches!(
            store.issue_session("", 0),
            Err(WebAuthError::InvalidLabel)
        ));
        let ticket = store.create_pairing_ticket(100).expect("ticket");
        assert!(matches!(
            store.redeem_pairing_ticket(&ticket.code, "Browser", 100 + PAIRING_TICKET_SECONDS),
            Err(WebAuthError::TicketExpired)
        ));
    }
}
