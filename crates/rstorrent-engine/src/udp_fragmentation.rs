//! Safe operating-system fragmentation-policy capability checks.

use std::io;

use tokio::net::UdpSocket;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Ipv4FragmentationProtectionStatus {
    Verified,
    VerificationFailed,
    #[default]
    UnsupportedPlatform,
}

#[cfg(any(target_os = "android", target_os = "linux"))]
pub(crate) fn verify_ipv4_fragmentation_protection(
    socket: &UdpSocket,
) -> io::Result<Ipv4FragmentationProtectionStatus> {
    use rustix::net::sockopt::{Ipv4PathMtuDiscovery, ip_mtu_discover, set_ip_mtu_discover};

    let prior = match ip_mtu_discover(socket) {
        Ok(prior) => prior,
        Err(_) => return Ok(Ipv4FragmentationProtectionStatus::VerificationFailed),
    };
    if set_ip_mtu_discover(socket, Ipv4PathMtuDiscovery::PROBE).is_err() {
        return match ip_mtu_discover(socket) {
            Ok(observed) if observed == prior => {
                Ok(Ipv4FragmentationProtectionStatus::VerificationFailed)
            }
            Ok(_) => Err(io::Error::other(
                "failed IPv4 fragmentation-policy change altered the prior value",
            )),
            Err(error) => Err(io::Error::new(
                io::Error::from(error).kind(),
                "failed to verify IPv4 fragmentation policy after a rejected change",
            )),
        };
    }
    let protected = ip_mtu_discover(socket).map_err(io::Error::from);
    let restored = set_ip_mtu_discover(socket, prior).map_err(io::Error::from);
    let observed_restore = ip_mtu_discover(socket).map_err(io::Error::from);

    if let Err(error) = restored {
        return Err(io::Error::new(
            error.kind(),
            format!("failed to restore IPv4 fragmentation policy: {error}"),
        ));
    }
    let observed_restore = observed_restore?;
    if observed_restore != prior {
        return Err(io::Error::other(
            "restored IPv4 fragmentation policy does not match its prior value",
        ));
    }
    match protected {
        Ok(value) if value == Ipv4PathMtuDiscovery::PROBE => {
            Ok(Ipv4FragmentationProtectionStatus::Verified)
        }
        Ok(_) | Err(_) => Ok(Ipv4FragmentationProtectionStatus::VerificationFailed),
    }
}

#[cfg(not(any(target_os = "android", target_os = "linux")))]
pub(crate) fn verify_ipv4_fragmentation_protection(
    _socket: &UdpSocket,
) -> io::Result<Ipv4FragmentationProtectionStatus> {
    Ok(Ipv4FragmentationProtectionStatus::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

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

    #[cfg(not(any(target_os = "android", target_os = "linux")))]
    #[tokio::test]
    async fn unsupported_platform_fails_closed() {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        assert_eq!(
            verify_ipv4_fragmentation_protection(&socket).unwrap(),
            Ipv4FragmentationProtectionStatus::UnsupportedPlatform
        );
    }
}
