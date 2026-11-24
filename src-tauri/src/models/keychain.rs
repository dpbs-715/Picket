//! macOS Keychain storage for model API credentials via Apple Security.framework.
//! Never executes external CLI child processes with secrets in argv, and supports an
//! isolated `.keychain-db` file for automated tests and `--foundation-ui-smoke` so tests
//! exercise the real macOS Keychain engine without touching the developer's login keychain.
use std::{
    ffi::{CString, c_char, c_void},
    path::{Path, PathBuf},
    ptr,
    sync::{Arc, Mutex},
};

const KEYCHAIN_SERVICE: &str = "app.picket.desktop.model-credentials";
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const ERR_SEC_SUCCESS: i32 = 0;
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
const ERR_SEC_DUPLICATE_ITEM: i32 = -25299;

type CFTypeRef = *const c_void;
type CFStringRef = *const c_void;
type CFDataRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFArrayRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFIndex = isize;
type OSStatus = i32;

#[repr(C)]
struct OpaqueCallbacks([usize; 8]);

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: CFTypeRef;
    static kCFTypeDictionaryKeyCallBacks: OpaqueCallbacks;
    static kCFTypeDictionaryValueCallBacks: OpaqueCallbacks;
    static kCFTypeArrayCallBacks: OpaqueCallbacks;

    fn CFRelease(cf: CFTypeRef);
    fn CFStringCreateWithBytes(
        alloc: CFAllocatorRef,
        bytes: *const u8,
        num_bytes: CFIndex,
        encoding: u32,
        is_external_representation: u8,
    ) -> CFStringRef;
    fn CFDataCreate(alloc: CFAllocatorRef, bytes: *const u8, length: CFIndex) -> CFDataRef;
    fn CFDataGetBytePtr(the_data: CFDataRef) -> *const u8;
    fn CFDataGetLength(the_data: CFDataRef) -> CFIndex;
    fn CFDictionaryCreate(
        allocator: CFAllocatorRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        num_values: CFIndex,
        key_call_backs: *const c_void,
        value_call_backs: *const c_void,
    ) -> CFDictionaryRef;
    fn CFArrayCreate(
        allocator: CFAllocatorRef,
        values: *const CFTypeRef,
        num_values: CFIndex,
        call_backs: *const c_void,
    ) -> CFArrayRef;
}

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    static kSecClass: CFStringRef;
    static kSecClassGenericPassword: CFStringRef;
    static kSecAttrService: CFStringRef;
    static kSecAttrAccount: CFStringRef;
    static kSecValueData: CFStringRef;
    static kSecReturnData: CFStringRef;
    static kSecMatchLimit: CFStringRef;
    static kSecMatchLimitOne: CFTypeRef;
    static kSecUseKeychain: CFStringRef;
    static kSecMatchSearchList: CFStringRef;

    fn SecItemAdd(attributes: CFDictionaryRef, result: *mut CFTypeRef) -> OSStatus;
    fn SecItemCopyMatching(query: CFDictionaryRef, result: *mut CFTypeRef) -> OSStatus;
    fn SecItemUpdate(query: CFDictionaryRef, attributes_to_update: CFDictionaryRef) -> OSStatus;
    fn SecItemDelete(query: CFDictionaryRef) -> OSStatus;

    fn SecKeychainCreate(
        path_name: *const c_char,
        password_length: u32,
        password: *const c_void,
        prompt_user: u8,
        initial_access: CFTypeRef,
        keychain_ref: *mut CFTypeRef,
    ) -> OSStatus;
    fn SecKeychainOpen(path_name: *const c_char, keychain_ref: *mut CFTypeRef) -> OSStatus;
    fn SecKeychainUnlock(
        keychain: CFTypeRef,
        password_length: u32,
        password: *const c_void,
        use_password: u8,
    ) -> OSStatus;
}

struct CfOwned(CFTypeRef);

impl CfOwned {
    fn from_raw(ptr: CFTypeRef, label: &str) -> Result<Self, String> {
        if ptr.is_null() {
            Err(format!("Failed to allocate Keychain object ({label})"))
        } else {
            Ok(Self(ptr))
        }
    }

    fn as_ptr(&self) -> CFTypeRef {
        self.0
    }
}

impl Drop for CfOwned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { CFRelease(self.0) };
        }
    }
}

fn cf_string(text: &str) -> Result<CfOwned, String> {
    let raw = unsafe {
        CFStringCreateWithBytes(
            ptr::null(),
            text.as_ptr(),
            text.len() as CFIndex,
            K_CF_STRING_ENCODING_UTF8,
            0,
        )
    };
    CfOwned::from_raw(raw, "CFString")
}

fn cf_data(bytes: &[u8]) -> Result<CfOwned, String> {
    let raw = unsafe { CFDataCreate(ptr::null(), bytes.as_ptr(), bytes.len() as CFIndex) };
    CfOwned::from_raw(raw, "CFData")
}

fn cf_dictionary(pairs: &[(CFTypeRef, CFTypeRef)]) -> Result<CfOwned, String> {
    let keys: Vec<CFTypeRef> = pairs.iter().map(|(k, _)| *k).collect();
    let values: Vec<CFTypeRef> = pairs.iter().map(|(_, v)| *v).collect();
    let raw = unsafe {
        CFDictionaryCreate(
            ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            pairs.len() as CFIndex,
            (&raw const kCFTypeDictionaryKeyCallBacks).cast(),
            (&raw const kCFTypeDictionaryValueCallBacks).cast(),
        )
    };
    CfOwned::from_raw(raw, "CFDictionary")
}

fn cf_array(items: &[CFTypeRef]) -> Result<CfOwned, String> {
    let raw = unsafe {
        CFArrayCreate(
            ptr::null(),
            items.as_ptr(),
            items.len() as CFIndex,
            (&raw const kCFTypeArrayCallBacks).cast(),
        )
    };
    CfOwned::from_raw(raw, "CFArray")
}

#[derive(Clone)]
struct CustomKeychainConfig {
    path: PathBuf,
    password: String,
}

#[derive(Clone, Default)]
pub struct CredentialVault {
    custom: Arc<Mutex<Option<CustomKeychainConfig>>>,
    io_lock: Arc<Mutex<()>>,
}

impl CredentialVault {
    pub fn with_keychain_file(path: PathBuf, password: impl Into<String>) -> Self {
        Self {
            custom: Arc::new(Mutex::new(Some(CustomKeychainConfig {
                path,
                password: password.into(),
            }))),
            io_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn set_keychain_file(
        &self,
        path: PathBuf,
        password: impl Into<String>,
    ) -> Result<(), String> {
        *self
            .custom
            .lock()
            .map_err(|_| "Keychain configuration lock is unavailable")? =
            Some(CustomKeychainConfig {
                path,
                password: password.into(),
            });
        Ok(())
    }

    fn open_custom_keychain(&self) -> Result<Option<CfOwned>, String> {
        let Some(config) = self
            .custom
            .lock()
            .map_err(|_| "Keychain configuration lock is unavailable")?
            .clone()
        else {
            return Ok(None);
        };
        if let Some(parent) = config.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| "Cannot create isolated Keychain directory".to_string())?;
        }
        let path_str = config
            .path
            .to_str()
            .ok_or("Keychain path contains invalid UTF-8")?;
        let c_path = CString::new(path_str).map_err(|_| "Keychain path contains NUL bytes")?;
        let pw_bytes = config.password.as_bytes();
        let mut kc: CFTypeRef = ptr::null();
        if Path::new(&config.path).exists() {
            let open_status = unsafe { SecKeychainOpen(c_path.as_ptr(), &mut kc) };
            if open_status != ERR_SEC_SUCCESS || kc.is_null() {
                return Err(format!(
                    "Cannot open macOS Keychain file (status {open_status})"
                ));
            }
            let owned = CfOwned(kc);
            let unlock_status = unsafe {
                SecKeychainUnlock(
                    owned.as_ptr(),
                    pw_bytes.len() as u32,
                    pw_bytes.as_ptr().cast(),
                    1,
                )
            };
            if unlock_status != ERR_SEC_SUCCESS {
                return Err(format!(
                    "Cannot unlock macOS Keychain (status {unlock_status})"
                ));
            }
            Ok(Some(owned))
        } else {
            let create_status = unsafe {
                SecKeychainCreate(
                    c_path.as_ptr(),
                    pw_bytes.len() as u32,
                    pw_bytes.as_ptr().cast(),
                    0,
                    ptr::null(),
                    &mut kc,
                )
            };
            if create_status != ERR_SEC_SUCCESS || kc.is_null() {
                return Err(format!(
                    "Cannot initialize macOS Keychain file (status {create_status})"
                ));
            }
            Ok(Some(CfOwned(kc)))
        }
    }

    pub fn put(&self, account: &str, secret: &str) -> Result<(), String> {
        let _guard = self
            .io_lock
            .lock()
            .map_err(|_| "Keychain mutex is unavailable")?;
        let custom_kc = self.open_custom_keychain()?;
        let service_cf = cf_string(KEYCHAIN_SERVICE)?;
        let account_cf = cf_string(account)?;
        let value_cf = cf_data(secret.as_bytes())?;

        let mut add_pairs = vec![
            (unsafe { kSecClass }, unsafe { kSecClassGenericPassword }),
            (unsafe { kSecAttrService }, service_cf.as_ptr()),
            (unsafe { kSecAttrAccount }, account_cf.as_ptr()),
            (unsafe { kSecValueData }, value_cf.as_ptr()),
        ];
        if let Some(kc) = &custom_kc {
            add_pairs.push((unsafe { kSecUseKeychain }, kc.as_ptr()));
        }
        let add_dict = cf_dictionary(&add_pairs)?;
        let status = unsafe { SecItemAdd(add_dict.as_ptr(), ptr::null_mut()) };
        if status == ERR_SEC_SUCCESS {
            return Ok(());
        }
        if status == ERR_SEC_DUPLICATE_ITEM {
            let search_list = custom_kc
                .as_ref()
                .map(|kc| cf_array(&[kc.as_ptr()]))
                .transpose()?;
            let mut query_pairs = vec![
                (unsafe { kSecClass }, unsafe { kSecClassGenericPassword }),
                (unsafe { kSecAttrService }, service_cf.as_ptr()),
                (unsafe { kSecAttrAccount }, account_cf.as_ptr()),
            ];
            if let Some(list) = &search_list {
                query_pairs.push((unsafe { kSecMatchSearchList }, list.as_ptr()));
            }
            let query_dict = cf_dictionary(&query_pairs)?;
            let update_dict = cf_dictionary(&[(unsafe { kSecValueData }, value_cf.as_ptr())])?;
            let update_status = unsafe { SecItemUpdate(query_dict.as_ptr(), update_dict.as_ptr()) };
            if update_status == ERR_SEC_SUCCESS {
                return Ok(());
            }
            return Err(format!(
                "Cannot update model API key in macOS Keychain (status {update_status}); model configuration was not saved."
            ));
        }
        Err(format!(
            "Cannot store model API key in macOS Keychain (status {status}); model and key were not saved."
        ))
    }

    pub fn get(&self, account: &str) -> Result<Option<String>, String> {
        let _guard = self
            .io_lock
            .lock()
            .map_err(|_| "Keychain mutex is unavailable")?;
        let custom_kc = self.open_custom_keychain()?;
        let search_list = custom_kc
            .as_ref()
            .map(|kc| cf_array(&[kc.as_ptr()]))
            .transpose()?;
        let service_cf = cf_string(KEYCHAIN_SERVICE)?;
        let account_cf = cf_string(account)?;

        let mut query_pairs = vec![
            (unsafe { kSecClass }, unsafe { kSecClassGenericPassword }),
            (unsafe { kSecAttrService }, service_cf.as_ptr()),
            (unsafe { kSecAttrAccount }, account_cf.as_ptr()),
            (unsafe { kSecReturnData }, unsafe { kCFBooleanTrue }),
            (unsafe { kSecMatchLimit }, unsafe { kSecMatchLimitOne }),
        ];
        if let Some(list) = &search_list {
            query_pairs.push((unsafe { kSecMatchSearchList }, list.as_ptr()));
        }
        let query_dict = cf_dictionary(&query_pairs)?;
        let mut result: CFTypeRef = ptr::null();
        let status = unsafe { SecItemCopyMatching(query_dict.as_ptr(), &mut result) };
        if status == ERR_SEC_ITEM_NOT_FOUND {
            return Ok(None);
        }
        if status != ERR_SEC_SUCCESS || result.is_null() {
            return Err(format!(
                "Failed to read credential from macOS Keychain (status {status})"
            ));
        }
        let data = CfOwned(result);
        let len = unsafe { CFDataGetLength(data.as_ptr()) };
        let ptr_bytes = unsafe { CFDataGetBytePtr(data.as_ptr()) };
        if len < 0 || (len > 0 && ptr_bytes.is_null()) {
            return Err("Credential data returned by macOS Keychain is corrupted".into());
        }
        let slice = if len == 0 {
            &[]
        } else {
            unsafe { std::slice::from_raw_parts(ptr_bytes, len as usize) }
        };
        let text = String::from_utf8(slice.to_vec())
            .map_err(|_| "Credential stored in macOS Keychain is not valid UTF-8".to_string())?;
        Ok(Some(text))
    }

    pub fn delete(&self, account: &str) -> Result<(), String> {
        let _guard = self
            .io_lock
            .lock()
            .map_err(|_| "Keychain mutex is unavailable")?;
        let custom_kc = self.open_custom_keychain()?;
        let search_list = custom_kc
            .as_ref()
            .map(|kc| cf_array(&[kc.as_ptr()]))
            .transpose()?;
        let service_cf = cf_string(KEYCHAIN_SERVICE)?;
        let account_cf = cf_string(account)?;

        let mut query_pairs = vec![
            (unsafe { kSecClass }, unsafe { kSecClassGenericPassword }),
            (unsafe { kSecAttrService }, service_cf.as_ptr()),
            (unsafe { kSecAttrAccount }, account_cf.as_ptr()),
        ];
        if let Some(list) = &search_list {
            query_pairs.push((unsafe { kSecMatchSearchList }, list.as_ptr()));
        }
        let query_dict = cf_dictionary(&query_pairs)?;
        let status = unsafe { SecItemDelete(query_dict.as_ptr()) };
        if status == ERR_SEC_SUCCESS || status == ERR_SEC_ITEM_NOT_FOUND {
            Ok(())
        } else {
            Err(format!(
                "Cannot delete model credential from macOS Keychain (status {status})"
            ))
        }
    }
}
