//! Antivirus and firewall status from Windows Security Center (read-only;
//! spec §2.1 "Security check": report and deep-link, no engine of our own).

use windows::Win32::System::SecurityCenter::{
    WSC_SECURITY_PROVIDER, WSC_SECURITY_PROVIDER_ANTIVIRUS, WSC_SECURITY_PROVIDER_FIREWALL,
    WSC_SECURITY_PROVIDER_HEALTH, WSC_SECURITY_PROVIDER_HEALTH_GOOD, WscGetSecurityProviderHealth,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provider {
    Antivirus,
    Firewall,
}

/// `Some(true)` when Security Center reports the provider healthy, `None` if
/// it can't be asked (service stopped, Windows Server without WSC).
pub fn healthy(p: Provider) -> Option<bool> {
    let which: WSC_SECURITY_PROVIDER = match p {
        Provider::Antivirus => WSC_SECURITY_PROVIDER_ANTIVIRUS,
        Provider::Firewall => WSC_SECURITY_PROVIDER_FIREWALL,
    };
    let mut h = WSC_SECURITY_PROVIDER_HEALTH::default();
    // SAFETY: writes one enum value into `h`.
    unsafe { WscGetSecurityProviderHealth(which.0 as u32, &mut h) }.ok()?;
    Some(h == WSC_SECURITY_PROVIDER_HEALTH_GOOD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn security_center_answers_on_a_normal_desktop() {
        // A desktop Windows always has Security Center; we only check it answers.
        assert!(healthy(Provider::Antivirus).is_some());
        assert!(healthy(Provider::Firewall).is_some());
    }
}
