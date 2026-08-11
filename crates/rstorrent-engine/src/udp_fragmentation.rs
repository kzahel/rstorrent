//! Safe operating-system fragmentation-policy transactions.

use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::panic::{AssertUnwindSafe, catch_unwind};

use tokio::net::UdpSocket;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Ipv4FragmentationProtectionStatus {
    Verified,
    VerificationFailed,
    #[default]
    UnsupportedPlatform,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Ipv4ProtectedSendResult {
    Sent(usize),
    WouldBlock,
    MessageTooLarge,
}

#[derive(Debug)]
pub(crate) enum Ipv4ProtectedSendError {
    Policy(io::Error),
    Send(io::Error),
    RestoreUncertain(io::Error),
    OperationPanicked,
}

impl Ipv4ProtectedSendError {
    pub(crate) fn restore_is_uncertain(&self) -> bool {
        matches!(self, Self::RestoreUncertain(_))
    }
}

impl fmt::Display for Ipv4ProtectedSendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Policy(error) => write!(formatter, "fragmentation policy failed: {error}"),
            Self::Send(error) => write!(formatter, "protected UDP send failed: {error}"),
            Self::RestoreUncertain(error) => {
                write!(
                    formatter,
                    "fragmentation-policy restoration is uncertain: {error}"
                )
            }
            Self::OperationPanicked => {
                formatter.write_str("protected UDP send panicked after policy restoration")
            }
        }
    }
}

trait Ipv4FragmentationPolicy {
    type Value: Copy + fmt::Debug + Eq;

    fn current(&self) -> io::Result<Self::Value>;
    fn protected(&self) -> Self::Value;
    fn set(&self, value: Self::Value) -> io::Result<()>;
}

fn with_ipv4_fragmentation_protection<P, F, T>(
    policy: &P,
    operation: F,
) -> Result<T, Ipv4ProtectedSendError>
where
    P: Ipv4FragmentationPolicy,
    F: FnOnce() -> Result<T, io::Error>,
{
    let prior = policy.current().map_err(Ipv4ProtectedSendError::Policy)?;
    let protected = policy.protected();
    if let Err(error) = policy.set(protected) {
        return policy_failure_after_possible_change(policy, prior, error);
    }
    match policy.current() {
        Ok(observed) if observed == protected => {}
        Ok(observed) => {
            let error = io::Error::other(format!(
                "protected fragmentation policy was not installed: observed {observed:?}"
            ));
            return restore_then_policy_failure(policy, prior, error);
        }
        Err(error) => return restore_then_policy_failure(policy, prior, error),
    }

    let result = catch_unwind(AssertUnwindSafe(operation));
    restore_exact(policy, prior)?;
    match result {
        Ok(result) => result.map_err(Ipv4ProtectedSendError::Send),
        Err(_) => Err(Ipv4ProtectedSendError::OperationPanicked),
    }
}

fn policy_failure_after_possible_change<P, T>(
    policy: &P,
    prior: P::Value,
    policy_error: io::Error,
) -> Result<T, Ipv4ProtectedSendError>
where
    P: Ipv4FragmentationPolicy,
{
    match policy.current() {
        Ok(observed) if observed == prior => Err(Ipv4ProtectedSendError::Policy(policy_error)),
        Ok(_) | Err(_) => restore_then_policy_failure(policy, prior, policy_error),
    }
}

fn restore_then_policy_failure<P, T>(
    policy: &P,
    prior: P::Value,
    policy_error: io::Error,
) -> Result<T, Ipv4ProtectedSendError>
where
    P: Ipv4FragmentationPolicy,
{
    restore_exact(policy, prior)?;
    Err(Ipv4ProtectedSendError::Policy(policy_error))
}

fn restore_exact<P>(policy: &P, prior: P::Value) -> Result<(), Ipv4ProtectedSendError>
where
    P: Ipv4FragmentationPolicy,
{
    if let Err(error) = policy.set(prior) {
        return Err(Ipv4ProtectedSendError::RestoreUncertain(io::Error::new(
            error.kind(),
            format!("failed to restore prior value {prior:?}: {error}"),
        )));
    }
    match policy.current() {
        Ok(observed) if observed == prior => Ok(()),
        Ok(observed) => Err(Ipv4ProtectedSendError::RestoreUncertain(io::Error::other(
            format!("restored value {observed:?} does not match prior value {prior:?}"),
        ))),
        Err(error) => Err(Ipv4ProtectedSendError::RestoreUncertain(io::Error::new(
            error.kind(),
            format!("failed to verify restored value {prior:?}: {error}"),
        ))),
    }
}

#[cfg(any(target_os = "android", target_os = "linux"))]
struct SocketIpv4FragmentationPolicy<'a>(&'a UdpSocket);

#[cfg(any(target_os = "android", target_os = "linux"))]
impl Ipv4FragmentationPolicy for SocketIpv4FragmentationPolicy<'_> {
    type Value = rustix::net::sockopt::Ipv4PathMtuDiscovery;

    fn current(&self) -> io::Result<Self::Value> {
        rustix::net::sockopt::ip_mtu_discover(self.0).map_err(io::Error::from)
    }

    fn protected(&self) -> Self::Value {
        Self::Value::PROBE
    }

    fn set(&self, value: Self::Value) -> io::Result<()> {
        rustix::net::sockopt::set_ip_mtu_discover(self.0, value).map_err(io::Error::from)
    }
}

#[cfg(target_os = "macos")]
struct SocketIpv4FragmentationPolicy<'a>(&'a UdpSocket);

#[cfg(target_os = "macos")]
impl Ipv4FragmentationPolicy for SocketIpv4FragmentationPolicy<'_> {
    type Value = bool;

    fn current(&self) -> io::Result<Self::Value> {
        use dontfrag::UdpSocketExt;
        self.0.dontfrag_v4()
    }

    fn protected(&self) -> Self::Value {
        true
    }

    fn set(&self, value: Self::Value) -> io::Result<()> {
        use dontfrag::UdpSocketExt;
        self.0.set_dontfrag_v4(value)
    }
}

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos"))]
pub(crate) fn verify_ipv4_fragmentation_protection(
    socket: &UdpSocket,
) -> io::Result<Ipv4FragmentationProtectionStatus> {
    let policy = SocketIpv4FragmentationPolicy(socket);
    match with_ipv4_fragmentation_protection(&policy, || Ok(())) {
        Ok(()) => Ok(Ipv4FragmentationProtectionStatus::Verified),
        Err(Ipv4ProtectedSendError::Policy(_)) => {
            Ok(Ipv4FragmentationProtectionStatus::VerificationFailed)
        }
        Err(Ipv4ProtectedSendError::RestoreUncertain(error)) => Err(error),
        Err(Ipv4ProtectedSendError::Send(_)) | Err(Ipv4ProtectedSendError::OperationPanicked) => {
            unreachable!("capability verification performs an infallible operation")
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "linux", target_os = "macos")))]
pub(crate) fn verify_ipv4_fragmentation_protection(
    _socket: &UdpSocket,
) -> io::Result<Ipv4FragmentationProtectionStatus> {
    Ok(Ipv4FragmentationProtectionStatus::UnsupportedPlatform)
}

#[cfg(any(target_os = "android", target_os = "linux", target_os = "macos"))]
pub(crate) fn try_send_ipv4_fragmentation_protected(
    socket: &UdpSocket,
    bytes: &[u8],
    target: SocketAddr,
) -> Result<Ipv4ProtectedSendResult, Ipv4ProtectedSendError> {
    let policy = SocketIpv4FragmentationPolicy(socket);
    with_ipv4_fragmentation_protection(&policy, || match socket.try_send_to(bytes, target) {
        Ok(length) => Ok(Ipv4ProtectedSendResult::Sent(length)),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            Ok(Ipv4ProtectedSendResult::WouldBlock)
        }
        Err(error) if is_message_too_large(&error) => Ok(Ipv4ProtectedSendResult::MessageTooLarge),
        Err(error) => Err(error),
    })
}

#[cfg(not(any(target_os = "android", target_os = "linux", target_os = "macos")))]
pub(crate) fn try_send_ipv4_fragmentation_protected(
    _socket: &UdpSocket,
    _bytes: &[u8],
    _target: SocketAddr,
) -> Result<Ipv4ProtectedSendResult, Ipv4ProtectedSendError> {
    Err(Ipv4ProtectedSendError::Policy(io::Error::new(
        io::ErrorKind::Unsupported,
        "IPv4 fragmentation protection is unavailable on this platform",
    )))
}

pub(crate) fn is_message_too_large(error: &io::Error) -> bool {
    error
        .raw_os_error()
        .is_some_and(|raw| rustix::io::Errno::from_raw_os_error(raw) == rustix::io::Errno::MSGSIZE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::net::Ipv4Addr;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FakeValue {
        Prior,
        Protected,
        Contaminated,
    }

    struct FakePolicy {
        value: Cell<FakeValue>,
        set_calls: Cell<usize>,
        fail_set_call: Cell<Option<usize>>,
        lie_after_restore: Cell<bool>,
        observations: RefCell<Vec<FakeValue>>,
    }

    impl FakePolicy {
        fn healthy() -> Self {
            Self {
                value: Cell::new(FakeValue::Prior),
                set_calls: Cell::new(0),
                fail_set_call: Cell::new(None),
                lie_after_restore: Cell::new(false),
                observations: RefCell::new(Vec::new()),
            }
        }
    }

    impl Ipv4FragmentationPolicy for FakePolicy {
        type Value = FakeValue;

        fn current(&self) -> io::Result<Self::Value> {
            let value = self.value.get();
            self.observations.borrow_mut().push(value);
            Ok(value)
        }

        fn protected(&self) -> Self::Value {
            FakeValue::Protected
        }

        fn set(&self, value: Self::Value) -> io::Result<()> {
            let call = self.set_calls.get().saturating_add(1);
            self.set_calls.set(call);
            if self.fail_set_call.get() == Some(call) {
                self.value.set(FakeValue::Contaminated);
                return Err(io::Error::other("injected set failure"));
            }
            if value == FakeValue::Prior && self.lie_after_restore.get() {
                self.value.set(FakeValue::Contaminated);
            } else {
                self.value.set(value);
            }
            Ok(())
        }
    }

    #[test]
    fn transaction_restores_before_returning_every_operation_result() {
        for operation in [Ok(7), Err(io::Error::other("send failure"))] {
            let policy = FakePolicy::healthy();
            let result = with_ipv4_fragmentation_protection(&policy, || operation);
            assert_eq!(policy.value.get(), FakeValue::Prior);
            assert_eq!(policy.set_calls.get(), 2);
            match result {
                Ok(7) | Err(Ipv4ProtectedSendError::Send(_)) => {}
                other => panic!("unexpected result: {other:?}"),
            }
        }
    }

    #[test]
    fn transaction_restores_before_reporting_an_operation_panic() {
        let policy = FakePolicy::healthy();
        let result = with_ipv4_fragmentation_protection(&policy, || -> io::Result<()> {
            panic!("injected send panic")
        });
        assert!(matches!(
            result,
            Err(Ipv4ProtectedSendError::OperationPanicked)
        ));
        assert_eq!(policy.value.get(), FakeValue::Prior);
        assert_eq!(policy.set_calls.get(), 2);
    }

    #[test]
    fn uncertain_restore_is_distinct_and_never_claimed_clean() {
        let policy = FakePolicy::healthy();
        policy.lie_after_restore.set(true);
        let error = with_ipv4_fragmentation_protection(&policy, || Ok(()))
            .expect_err("mismatched restore must fail");
        assert!(error.restore_is_uncertain());
        assert_eq!(policy.value.get(), FakeValue::Contaminated);
    }

    #[test]
    fn a_failed_protected_set_repairs_a_possible_partial_change() {
        let policy = FakePolicy::healthy();
        policy.fail_set_call.set(Some(1));
        let error = with_ipv4_fragmentation_protection(&policy, || Ok(()))
            .expect_err("failed set must fail");
        assert!(matches!(error, Ipv4ProtectedSendError::Policy(_)));
        assert_eq!(policy.value.get(), FakeValue::Prior);
        assert_eq!(policy.set_calls.get(), 2);
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    #[tokio::test]
    async fn verification_restores_the_exact_linux_policy() {
        use rustix::net::sockopt::{Ipv4PathMtuDiscovery, ip_mtu_discover, set_ip_mtu_discover};

        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        set_ip_mtu_discover(&socket, Ipv4PathMtuDiscovery::WANT).unwrap();
        let prior = ip_mtu_discover(&socket).unwrap();
        assert_eq!(
            verify_ipv4_fragmentation_protection(&socket).unwrap(),
            Ipv4FragmentationProtectionStatus::Verified
        );
        assert_eq!(ip_mtu_discover(&socket).unwrap(), prior);
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn verification_and_protected_send_restore_exact_macos_policy() {
        use dontfrag::UdpSocketExt;

        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let receiver = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        socket.set_dontfrag_v4(false).unwrap();
        let prior = socket.dontfrag_v4().unwrap();
        assert_eq!(
            verify_ipv4_fragmentation_protection(&socket).unwrap(),
            Ipv4FragmentationProtectionStatus::Verified
        );
        assert_eq!(socket.dontfrag_v4().unwrap(), prior);
        socket.writable().await.unwrap();
        assert_eq!(
            try_send_ipv4_fragmentation_protected(
                &socket,
                b"protected",
                receiver.local_addr().unwrap()
            )
            .unwrap(),
            Ipv4ProtectedSendResult::Sent(9)
        );
        assert_eq!(socket.dontfrag_v4().unwrap(), prior);
    }

    #[cfg(not(any(target_os = "android", target_os = "linux", target_os = "macos")))]
    #[tokio::test]
    async fn unsupported_platform_fails_closed() {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        assert_eq!(
            verify_ipv4_fragmentation_protection(&socket).unwrap(),
            Ipv4FragmentationProtectionStatus::UnsupportedPlatform
        );
    }
}
