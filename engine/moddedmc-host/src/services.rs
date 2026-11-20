//! Named, versioned C tables shared among separately loaded native mods.

use std::{
    collections::BTreeMap,
    ffi::c_void,
    ptr,
    sync::{LazyLock, Mutex, PoisonError},
};

use moddedmc_api::abi::{self, Reg, Str, code};

use crate::{HOST, native};

pub struct Registration {
    pub name: String,
    pub version: u32,
    pub table: usize,
}

static TABLES: LazyLock<Mutex<BTreeMap<(String, u32), usize>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

fn tables() -> std::sync::MutexGuard<'static, BTreeMap<(String, u32), usize>> {
    TABLES.lock().unwrap_or_else(PoisonError::into_inner)
}

pub static SERVICES: abi::ServicesV1 = abi::ServicesV1 {
    size: size_of::<abi::ServicesV1>(),
    publish,
};

fn valid_service_name(name: &str, owner: &str) -> bool {
    let Some((namespace, path)) = name.split_once(':') else {
        return false;
    };
    namespace == owner
        && !path.is_empty()
        && path.len() <= 128
        && path
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-/".contains(&b))
}

unsafe extern "C" fn publish(reg: Reg, name: Str, version: u32, table: *const c_void) -> i32 {
    if version == 0 || table.is_null() || !(table as usize).is_multiple_of(align_of::<usize>()) {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: the registration is valid only for a current mod registration.
    let staging = unsafe { native::staging(reg) };
    let owner = {
        let mods = super::lock(&HOST.mods);
        let Some(entry) = mods.get(staging.mod_idx) else {
            return code::INVALID_DEFINITION;
        };
        entry.module.id
    };
    // SAFETY: mods promise the borrowed service name stays alive for this call.
    let name = unsafe { name.str() };
    if !valid_service_name(name, owner) {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: native mods are trusted to hand the host a readable, permanently
    // allocated repr(C) table starting with its byte length.
    let size = unsafe { table.cast::<usize>().read() };
    if !(size_of::<usize>()..=(1 << 20)).contains(&size) {
        return code::INVALID_DEFINITION;
    }
    if staging
        .services
        .iter()
        .any(|entry| entry.name == name && entry.version == version)
        || tables().contains_key(&(name.to_owned(), version))
    {
        return code::DUPLICATE_ID;
    }
    staging.services.push(Registration {
        name: name.to_owned(),
        version,
        table: table as usize,
    });
    code::OK
}

pub fn commit(staged: Vec<Registration>) {
    let mut registry = tables();
    for entry in staged {
        registry.insert((entry.name, entry.version), entry.table);
    }
}

pub fn available(staged: &[Registration]) -> bool {
    let registry = tables();
    staged
        .iter()
        .all(|entry| !registry.contains_key(&(entry.name.clone(), entry.version)))
}

pub fn lookup(name: &str, version: u32) -> *const c_void {
    tables()
        .get(&(name.to_owned(), version))
        .copied()
        .map_or(ptr::null(), |address| address as *const c_void)
}

#[cfg(test)]
mod tests {
    use super::valid_service_name;

    #[test]
    fn publication_is_owner_scoped() {
        assert!(valid_service_name("create:kinetics", "create"));
        assert!(!valid_service_name("create:kinetics", "addon"));
        assert!(!valid_service_name("create", "create"));
        assert!(!valid_service_name("create:UPPER", "create"));
        assert!(!valid_service_name("create:", "create"));
        assert!(!valid_service_name("create:bad space", "create"));
    }
}
