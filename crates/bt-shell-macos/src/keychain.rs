//! The saved ssh passwords in the macOS Keychain: the real
//! body of [`bt_shell_common::ssh_route::PasswordStore`].
//!
//! - **What**: an *internet password* (`kSecClassInternetPassword`) — server
//!   the host, account the user, the port, protocol SSH — labelled
//!   `bateri — user@host:port`, so Keychain Access shows it under the host and
//!   finds it by "bateri". The login keychain, **not** the
//!   data-protection keychain (`kSecUseDataProtectionKeychain` is not set: that
//!   one wants a provisioning profile's entitlement).
//! - **Who**: only the application process. The askpass helper (the same
//!   binary, started by ssh) never reaches here — it asks the application over
//!   its socket ([`bt_shell_common::ssh_route::run_askpass`]).
//! - **Known limit**: an ad-hoc signed build changes its signature at
//!   every build, so macOS asks "bateri wants to use your confidential
//!   information" each time; a Developer ID build asks once.
//!
//! No test writes here: the login keychain is the user's and a write asks for
//! consent. The rules around it are tested against a memory store in
//! `ssh_route`.

use std::ptr::{self, NonNull};

use bt_shell_common::ssh_route::{Account, PasswordStore};
use objc2_core_foundation::{
    CFBoolean, CFData, CFDictionary, CFNumber, CFRetained, CFString, CFType,
};
use objc2_security::{
    SecItemAdd, SecItemCopyMatching, SecItemDelete, SecItemUpdate, kSecAttrAccount, kSecAttrLabel,
    kSecAttrPort, kSecAttrProtocol, kSecAttrProtocolSSH, kSecAttrServer, kSecClass,
    kSecClassInternetPassword, kSecMatchLimit, kSecMatchLimitOne, kSecReturnData, kSecValueData,
};

/// `errSecSuccess` (`SecBase.h`); the codes are named here rather than turning
/// on the `SecBase` header of `objc2-security` for three constants.
const SUCCESS: i32 = 0;
/// `errSecDuplicateItem`: the add found the item — the password is updated.
const DUPLICATE_ITEM: i32 = -25299;

/// The login keychain as the masters' password store.
pub(crate) struct Keychain;

/// The item's identity — class, server, account, port, protocol — plus
/// `extra` pairs: what a read, an update and a delete match on, and what an
/// add writes.
fn query(
    account: &Account,
    extra: &[(&'static CFString, &CFType)],
) -> CFRetained<CFDictionary<CFString, CFType>> {
    let host = CFString::from_str(&account.host);
    let user = CFString::from_str(&account.user);
    let port = CFNumber::new_i32(i32::from(account.port));
    // SAFETY: the `kSec*` statics are immutable constants Security.framework
    // exports; reading them has no precondition.
    let (class_key, server, user_key, port_key, protocol_key, class, protocol) = unsafe {
        (
            kSecClass,
            kSecAttrServer,
            kSecAttrAccount,
            kSecAttrPort,
            kSecAttrProtocol,
            kSecClassInternetPassword,
            kSecAttrProtocolSSH,
        )
    };
    let mut keys: Vec<&CFString> = vec![class_key, server, user_key, port_key, protocol_key];
    let mut values: Vec<&CFType> = vec![
        class.as_ref(),
        host.as_ref(),
        user.as_ref(),
        port.as_ref(),
        protocol.as_ref(),
    ];
    for &(key, value) in extra {
        keys.push(key);
        values.push(value);
    }
    CFDictionary::from_slices(&keys, &values)
}

impl PasswordStore for Keychain {
    fn read(&self, account: &Account) -> Option<String> {
        // SAFETY: immutable framework constants.
        let (limit, one, data_key) = unsafe { (kSecMatchLimit, kSecMatchLimitOne, kSecReturnData) };
        let yes: &CFType = CFBoolean::new(true).as_ref();
        let query = query(account, &[(limit, one.as_ref()), (data_key, yes)]);
        let mut result: *const CFType = ptr::null();
        // SAFETY: the query is a valid `CFDictionary` of `CFString` keys and
        // `CFType` values; `result` is a valid out pointer. On success it holds
        // a +1 reference (the "Copy" rule), taken over below.
        let status = unsafe { SecItemCopyMatching(query.as_opaque(), &mut result) };
        if status != SUCCESS {
            return None;
        }
        // SAFETY: a successful copy hands over one reference to a CF object.
        let object = unsafe { CFRetained::from_raw(NonNull::new(result.cast_mut())?) };
        let data = object.downcast::<CFData>().ok()?;
        String::from_utf8(data.to_vec()).ok()
    }

    fn write(&self, account: &Account, password: &str) {
        let data = CFData::from_bytes(password.as_bytes());
        let label = CFString::from_str(&account.label());
        // SAFETY: immutable framework constants.
        let (value_key, label_key) = unsafe { (kSecValueData, kSecAttrLabel) };
        let add = query(
            account,
            &[(value_key, data.as_ref()), (label_key, label.as_ref())],
        );
        // SAFETY: a valid attribute dictionary; no result is asked for (null).
        let status = unsafe { SecItemAdd(add.as_opaque(), ptr::null_mut()) };
        if status == DUPLICATE_ITEM {
            let matching = query(account, &[]);
            let change: CFRetained<CFDictionary<CFString, CFType>> = CFDictionary::from_slices(
                &[value_key, label_key],
                &[data.as_ref(), label.as_ref()],
            );
            // SAFETY: two valid dictionaries — the item's identity and the
            // attributes to change.
            let _ = unsafe { SecItemUpdate(matching.as_opaque(), change.as_opaque()) };
        }
    }

    fn delete(&self, account: &Account) {
        let matching = query(account, &[]);
        // SAFETY: a valid query dictionary.
        let _ = unsafe { SecItemDelete(matching.as_opaque()) };
    }

    fn contains(&self, account: &Account) -> bool {
        // SAFETY: immutable framework constants.
        let (limit, one) = unsafe { (kSecMatchLimit, kSecMatchLimitOne) };
        // No `kSecReturnData`: the attributes-only question asks for no consent.
        let query = query(account, &[(limit, one.as_ref())]);
        // SAFETY: a valid query; a null result asks only whether it matches.
        let status = unsafe { SecItemCopyMatching(query.as_opaque(), ptr::null_mut()) };
        status == SUCCESS
    }
}
