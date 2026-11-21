//! Whose an app is: the helper is run only when its signature is ours. The static check
//! of Security.framework (`SecStaticCodeCheckValidity`) against a requirement — "signed by
//! Apple's chain, and by our team" — over the app **at the path that will be run**, its nested
//! code and every architecture, strictly (a changed byte, a file added to the bundle and a
//! stripped signature each fail).
//!
//! Our team is the team of bateri's own signature: no identity is written into the code. A bateri
//! that has no team (an ad-hoc signed one, `cargo run`) cannot say whose the helper must be, so
//! it **refuses**; it never skips the check. A development copy (`make dev`) is the one case that
//! runs an app without it, and it says so by being a development copy (`browser_install::manager`).

use std::path::Path;
use std::ptr::{self, NonNull};

use objc2_core_foundation::{CFDictionary, CFRetained, CFString, CFType, CFURL, CFURLPathStyle};
use objc2_security::{
    SecCSFlags, SecCode, SecRequirement, SecStaticCode, kSecCSCheckAllArchitectures,
    kSecCSCheckNestedCode, kSecCSSigningInformation, kSecCSStrictValidate,
    kSecCodeInfoTeamIdentifier,
};

const SUCCESS: i32 = 0;

/// The requirement text for a team: Apple's chain with a Developer ID leaf of that team. The team
/// is ten letters and digits, and nothing else is let into the text.
pub fn requirement_for_team(team: &str) -> Result<String, String> {
    let well_formed = team.len() == 10
        && team
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit());
    if !well_formed {
        return Err(format!("{team:?} is not a team identifier"));
    }
    Ok(format!(
        "anchor apple generic and certificate leaf[subject.OU] = \"{team}\""
    ))
}

fn static_code(path: &Path) -> Result<CFRetained<SecStaticCode>, String> {
    let text = path.to_str().ok_or("the path is not UTF-8")?;
    let url = CFURL::with_file_system_path(
        None,
        Some(&CFString::from_str(text)),
        CFURLPathStyle::CFURLPOSIXPathStyle,
        true,
    )
    .ok_or("the path is not a file URL")?;
    let mut code: *const SecStaticCode = ptr::null();
    // SAFETY: `code` is a valid out-pointer; the call follows the Create rule (+1).
    let status = unsafe {
        SecStaticCode::create_with_path(&url, SecCSFlags::DefaultFlags, NonNull::from(&mut code))
    };
    if status != SUCCESS {
        return Err(format!(
            "{} cannot be read as code (status {status})",
            path.display()
        ));
    }
    let code = NonNull::new(code.cast_mut()).ok_or("no code was made")?;
    // SAFETY: a +1 reference the call just gave.
    Ok(unsafe { CFRetained::from_raw(code) })
}

/// Checks that the app at `app` meets `requirement` (text, `SecRequirementCreateWithString`).
pub fn verify(app: &Path, requirement: &str) -> Result<(), String> {
    let code = static_code(app)?;
    let text = CFString::from_str(requirement);
    let mut raw: *mut SecRequirement = ptr::null_mut();
    // SAFETY: `raw` is a valid out-pointer; the call follows the Create rule.
    let status = unsafe {
        SecRequirement::create_with_string(&text, SecCSFlags::DefaultFlags, NonNull::from(&mut raw))
    };
    let requirement = NonNull::new(raw)
        .filter(|_| status == SUCCESS)
        .ok_or_else(|| format!("the requirement is not valid (status {status})"))?;
    // SAFETY: a +1 reference the call just gave.
    let requirement: CFRetained<SecRequirement> = unsafe { CFRetained::from_raw(requirement) };
    let flags =
        SecCSFlags(kSecCSCheckAllArchitectures | kSecCSCheckNestedCode | kSecCSStrictValidate);
    // SAFETY: both references are alive for the call.
    let status = unsafe { SecStaticCode::check_validity(&code, flags, Some(&requirement)) };
    if status == SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "the signature of {} is not ours (status {status})",
            app.display()
        ))
    }
}

/// The team identifier of the running bateri, `None` when it is not signed by a team.
pub fn own_team() -> Option<String> {
    let mut code: *mut SecCode = ptr::null_mut();
    // SAFETY: `code` is a valid out-pointer; the call follows the Create rule.
    let status = unsafe { SecCode::copy_self(SecCSFlags::DefaultFlags, NonNull::from(&mut code)) };
    let code: CFRetained<SecCode> =
        NonNull::new(code)
            .filter(|_| status == SUCCESS)
            .map(|code| {
                // SAFETY: a +1 reference the call just gave.
                unsafe { CFRetained::from_raw(code) }
            })?;
    let mut fixed: *const SecStaticCode = ptr::null();
    // SAFETY: as above.
    let status = unsafe {
        SecCode::copy_static_code(&code, SecCSFlags::DefaultFlags, NonNull::from(&mut fixed))
    };
    let fixed: CFRetained<SecStaticCode> = NonNull::new(fixed.cast_mut())
        .filter(|_| status == SUCCESS)
        .map(|fixed| {
            // SAFETY: a +1 reference the call just gave.
            unsafe { CFRetained::from_raw(fixed) }
        })?;
    let mut information: *const CFDictionary = ptr::null();
    // SAFETY: `information` is a valid out-pointer; the call follows the Create rule.
    let status = unsafe {
        SecCode::copy_signing_information(
            &fixed,
            SecCSFlags(kSecCSSigningInformation),
            NonNull::from(&mut information),
        )
    };
    let information = NonNull::new(information.cast_mut()).filter(|_| status == SUCCESS)?;
    // SAFETY: a +1 reference the call just gave.
    let information: CFRetained<CFDictionary> = unsafe { CFRetained::from_raw(information) };
    // SAFETY: the signing information's keys are strings and its values are CF types.
    let information = unsafe { information.cast_unchecked::<CFString, CFType>() };
    // SAFETY: an immutable key Security.framework exports.
    let team = information.get(unsafe { kSecCodeInfoTeamIdentifier })?;
    Some(team.downcast_ref::<CFString>()?.to_string())
}
