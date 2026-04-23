//! Windows registry builder ops. Accessed from an [`Installer`] via
//! `i.registry()` which returns a short-lived [`Registry`] handle.

use anyhow::{Context, Result};

use crate::Installer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryHive {
    ClassesRoot,
    CurrentUser,
    LocalMachine,
    Users,
    CurrentConfig,
}

impl RegistryHive {
    fn to_hkey(self) -> winreg::HKEY {
        use winreg::enums::*;
        match self {
            RegistryHive::ClassesRoot => HKEY_CLASSES_ROOT,
            RegistryHive::CurrentUser => HKEY_CURRENT_USER,
            RegistryHive::LocalMachine => HKEY_LOCAL_MACHINE,
            RegistryHive::Users => HKEY_USERS,
            RegistryHive::CurrentConfig => HKEY_CURRENT_CONFIG,
        }
    }

    fn display(self) -> &'static str {
        match self {
            RegistryHive::ClassesRoot => "HKCR",
            RegistryHive::CurrentUser => "HKCU",
            RegistryHive::LocalMachine => "HKLM",
            RegistryHive::Users => "HKU",
            RegistryHive::CurrentConfig => "HKCC",
        }
    }
}

pub struct Registry<'i> {
    pub(crate) installer: &'i mut Installer,
}

impl<'i> Registry<'i> {
    /// Set a named value under `subkey`. Intermediate keys are created
    /// automatically. Value type is determined by `V` — any type
    /// implementing `winreg::types::ToRegValue` works (`String`, `&str`,
    /// `u32`, `u64`, `Vec<String>`, etc.).
    pub fn set<V: winreg::types::ToRegValue>(
        self,
        hive: RegistryHive,
        subkey: impl Into<String>,
        name: impl Into<String>,
        value: V,
    ) -> RegSetOp<'i> {
        RegSetOp {
            installer: self.installer,
            hive,
            subkey: subkey.into(),
            name: name.into(),
            value: value.to_reg_value(),
            weight: 1,
            status: None,
            log: None,
        }
    }

    /// Set the default (unnamed) value of `subkey`. Shorthand for
    /// `set(hive, subkey, "", value)`.
    pub fn default<V: winreg::types::ToRegValue>(
        self,
        hive: RegistryHive,
        subkey: impl Into<String>,
        value: V,
    ) -> RegSetOp<'i> {
        self.set(hive, subkey, "", value)
    }

    /// Read a registry value. Returns an error if the key or value
    /// doesn't exist or the stored type doesn't match `V`.
    pub fn get<V: winreg::types::FromRegValue>(
        &self,
        hive: RegistryHive,
        subkey: &str,
        name: &str,
    ) -> Result<V> {
        let hkey = winreg::RegKey::predef(hive.to_hkey());
        let k = hkey
            .open_subkey(subkey)
            .with_context(|| format!("failed to open {}\\{subkey}", hive.display()))?;
        k.get_value(name)
            .with_context(|| format!("failed to read {}\\{subkey}\\{name}", hive.display()))
    }

    /// Delete a subkey. Non-recursive by default (fails if the key has
    /// children); call `.recursive()` to delete children too. Missing
    /// keys are treated as success.
    pub fn remove(self, hive: RegistryHive, subkey: impl Into<String>) -> RegRemoveKeyOp<'i> {
        RegRemoveKeyOp {
            installer: self.installer,
            hive,
            subkey: subkey.into(),
            recursive: false,
            weight: 1,
            status: None,
            log: None,
        }
    }

    /// Delete a named value under `subkey`. Missing values are treated
    /// as success.
    pub fn delete(
        self,
        hive: RegistryHive,
        subkey: impl Into<String>,
        name: impl Into<String>,
    ) -> RegDeleteValueOp<'i> {
        RegDeleteValueOp {
            installer: self.installer,
            hive,
            subkey: subkey.into(),
            name: name.into(),
            weight: 1,
            status: None,
            log: None,
        }
    }
}

pub struct RegSetOp<'i> {
    installer: &'i mut Installer,
    hive: RegistryHive,
    subkey: String,
    name: String,
    value: winreg::RegValue,
    weight: u32,
    status: Option<String>,
    log: Option<String>,
}

impl<'i> RegSetOp<'i> {
    pub fn status(mut self, s: impl Into<String>) -> Self {
        self.status = Some(s.into());
        self
    }
    pub fn log(mut self, s: impl Into<String>) -> Self {
        self.log = Some(s.into());
        self
    }
    pub fn weight(mut self, w: u32) -> Self {
        self.weight = w;
        self
    }
    pub fn install(self) -> Result<()> {
        self.installer.check_cancelled()?;
        self.installer.emit_status(&self.status);
        self.installer.emit_log(&self.log);
        let hive = self.hive;
        let subkey = self.subkey;
        let name = self.name;
        let value = self.value;
        self.installer.run_weighted_step(self.weight, || {
            let hkey = winreg::RegKey::predef(hive.to_hkey());
            let (k, _) = hkey
                .create_subkey(&subkey)
                .with_context(|| format!("failed to create {}\\{subkey}", hive.display()))?;
            k.set_raw_value(&name, &value)
                .with_context(|| format!("failed to set {}\\{subkey}\\{name}", hive.display()))?;
            Ok(())
        })
    }
}

pub struct RegRemoveKeyOp<'i> {
    installer: &'i mut Installer,
    hive: RegistryHive,
    subkey: String,
    recursive: bool,
    weight: u32,
    status: Option<String>,
    log: Option<String>,
}

impl<'i> RegRemoveKeyOp<'i> {
    pub fn status(mut self, s: impl Into<String>) -> Self {
        self.status = Some(s.into());
        self
    }
    pub fn log(mut self, s: impl Into<String>) -> Self {
        self.log = Some(s.into());
        self
    }
    pub fn weight(mut self, w: u32) -> Self {
        self.weight = w;
        self
    }
    /// Recursively delete all child subkeys. Without this, removing a
    /// key that has children fails with an error.
    pub fn recursive(mut self) -> Self {
        self.recursive = true;
        self
    }
    pub fn install(self) -> Result<()> {
        self.installer.check_cancelled()?;
        self.installer.emit_status(&self.status);
        self.installer.emit_log(&self.log);
        let hive = self.hive;
        let subkey = self.subkey;
        let recursive = self.recursive;
        self.installer.run_weighted_step(self.weight, || {
            let hkey = winreg::RegKey::predef(hive.to_hkey());
            let res = if recursive {
                hkey.delete_subkey_all(&subkey)
            } else {
                hkey.delete_subkey(&subkey)
            };
            match res {
                Ok(_) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => {
                    Err(e).with_context(|| format!("failed to remove {}\\{subkey}", hive.display()))
                }
            }
        })
    }
}

pub struct RegDeleteValueOp<'i> {
    installer: &'i mut Installer,
    hive: RegistryHive,
    subkey: String,
    name: String,
    weight: u32,
    status: Option<String>,
    log: Option<String>,
}

impl<'i> RegDeleteValueOp<'i> {
    pub fn status(mut self, s: impl Into<String>) -> Self {
        self.status = Some(s.into());
        self
    }
    pub fn log(mut self, s: impl Into<String>) -> Self {
        self.log = Some(s.into());
        self
    }
    pub fn weight(mut self, w: u32) -> Self {
        self.weight = w;
        self
    }
    pub fn install(self) -> Result<()> {
        self.installer.check_cancelled()?;
        self.installer.emit_status(&self.status);
        self.installer.emit_log(&self.log);
        let hive = self.hive;
        let subkey = self.subkey;
        let name = self.name;
        self.installer.run_weighted_step(self.weight, || {
            let hkey = winreg::RegKey::predef(hive.to_hkey());
            let k = match hkey.open_subkey_with_flags(&subkey, winreg::enums::KEY_SET_VALUE) {
                Ok(k) => k,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => {
                    return Err(e)
                        .with_context(|| format!("failed to open {}\\{subkey}", hive.display()));
                }
            };
            match k.delete_value(&name) {
                Ok(_) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e).with_context(|| {
                    format!("failed to delete {}\\{subkey}\\{name}", hive.display())
                }),
            }
        })
    }
}
