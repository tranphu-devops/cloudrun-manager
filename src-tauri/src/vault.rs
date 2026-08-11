//! Vault mã hoá cho service account key.
//!
//! # Định dạng file
//!
//! ```text
//! offset  len  nội dung
//! 0       5    magic "CRCV1"
//! 5       1    version (=1)
//! 6       4    argon2 m_cost (KiB, u32 LE)
//! 10      4    argon2 t_cost (u32 LE)
//! 14      1    argon2 p_cost
//! 15      16   salt
//! 31      12   nonce
//! ---- 43 byte trên là header, đồng thời là AAD ----
//! 43      ..   ciphertext + tag (AES-256-GCM)
//! ```
//!
//! **Toàn bộ header là AAD.** Không làm vậy thì kẻ có file có thể hạ `m_cost` xuống 1
//! rồi brute-force passphrase với chi phí gần bằng 0. Có AAD thì sửa một byte header là
//! giải mã thất bại.
//!
//! Tham số Argon2 nằm trong file (không hardcode) nên sau này hạ/nâng được mà vẫn đọc
//! được vault cũ.
//!
//! # Bất biến
//!
//! - Khoá dẫn xuất và nội dung giải mã đều zeroize khi drop.
//! - Passphrase **không bao giờ** được lưu, kể cả dạng hash.
//! - Quên passphrase = không lấy lại được. Không có backdoor, không có recovery key.

use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

const MAGIC: &[u8; 5] = b"CRCV1";
const VERSION: u8 = 1;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const HEADER_LEN: usize = 5 + 1 + 4 + 4 + 1 + SALT_LEN + NONCE_LEN; // 43

/// Mặc định: 64 MiB / 3 lần / 1 luồng. Trên desktop mất ~200ms — người dùng không thấy
/// chậm, còn brute-force offline thì đắt.
const DEFAULT_M_COST: u32 = 65_536;
const DEFAULT_T_COST: u32 = 3;
const DEFAULT_P_COST: u32 = 1;

/// Passphrase ngắn hơn mức này thì Argon2 cũng không cứu được.
const MIN_PASSPHRASE_LEN: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            m_cost: DEFAULT_M_COST,
            t_cost: DEFAULT_T_COST,
            p_cost: DEFAULT_P_COST,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("Chưa có credential nào được lưu. Vào Cài đặt → Credential để import service account key.")]
    NotFound,

    #[error("Passphrase không đúng.")]
    WrongPassphrase,

    #[error("File vault bị hỏng hoặc đã bị sửa nội dung — không giải mã được. Nếu bạn còn file key gốc thì hãy import lại.")]
    Tampered,

    #[error("File vault không đúng định dạng (thiếu dấu hiệu nhận dạng ở đầu file).")]
    BadMagic,

    #[error("File vault thuộc phiên bản {0}, app này chỉ đọc được phiên bản {VERSION}.")]
    BadVersion(u8),

    #[error("File vault bị cắt ngắn ({0} byte, cần tối thiểu {HEADER_LEN}).")]
    Truncated(usize),

    #[error("Passphrase phải dài tối thiểu {MIN_PASSPHRASE_LEN} ký tự.")]
    PassphraseTooShort,

    #[error("Lỗi đọc/ghi file vault: {0}")]
    Io(String),

    #[error("Nội dung vault không đọc được: {0}")]
    Corrupt(String),

    #[error("Không có credential nào ở vị trí {0}.")]
    NoSuchIndex(usize),
}

type Result<T> = std::result::Result<T, VaultError>;

/// Nội dung bên trong vault (dạng rõ).
///
/// Để `credentials` là mảng ngay từ v2 dù UI chỉ dùng một cái: thêm SA thứ hai sau này
/// không phải đổi định dạng file và không phải viết migration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VaultContents {
    /// Nội dung thô của từng file SA key JSON.
    pub credentials: Vec<String>,
    pub active_index: usize,
}

impl VaultContents {
    pub fn active(&self) -> Option<&String> {
        self.credentials.get(self.active_index)
    }
}

impl Drop for VaultContents {
    fn drop(&mut self) {
        for c in self.credentials.iter_mut() {
            c.zeroize();
        }
    }
}

/// Vault đã mở khoá: giữ khoá dẫn xuất trong RAM để các thao tác sửa sau đó không phải
/// hỏi lại passphrase.
pub struct UnlockedVault {
    key: Zeroizing<[u8; 32]>,
    params: KdfParams,
    salt: [u8; SALT_LEN],
    pub contents: VaultContents,
}

impl std::fmt::Debug for UnlockedVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnlockedVault")
            .field("key", &"[redacted]")
            .field("params", &self.params)
            .field("credentials", &self.contents.credentials.len())
            .field("activeIndex", &self.contents.active_index)
            .finish()
    }
}

pub struct Vault {
    path: PathBuf,
}

impl Vault {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.is_file()
    }

    /// Tạo vault mới (ghi đè nếu đã có).
    pub fn create(
        &self,
        passphrase: &str,
        contents: &VaultContents,
        params: KdfParams,
    ) -> Result<UnlockedVault> {
        if passphrase.chars().count() < MIN_PASSPHRASE_LEN {
            return Err(VaultError::PassphraseTooShort);
        }

        let mut salt = [0u8; SALT_LEN];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        let key = derive_key(passphrase, &salt, params)?;

        self.write(&key, params, &salt, contents)?;

        Ok(UnlockedVault {
            key,
            params,
            salt,
            contents: contents.clone(),
        })
    }

    pub fn unlock(&self, passphrase: &str) -> Result<UnlockedVault> {
        let raw = std::fs::read(&self.path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => VaultError::NotFound,
            _ => VaultError::Io(e.to_string()),
        })?;

        if raw.len() < HEADER_LEN {
            return Err(VaultError::Truncated(raw.len()));
        }
        if &raw[0..5] != MAGIC {
            return Err(VaultError::BadMagic);
        }
        if raw[5] != VERSION {
            return Err(VaultError::BadVersion(raw[5]));
        }

        let params = KdfParams {
            m_cost: u32::from_le_bytes([raw[6], raw[7], raw[8], raw[9]]),
            t_cost: u32::from_le_bytes([raw[10], raw[11], raw[12], raw[13]]),
            p_cost: raw[14] as u32,
        };

        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&raw[15..15 + SALT_LEN]);
        let nonce_bytes = &raw[31..31 + NONCE_LEN];
        let header = &raw[..HEADER_LEN];
        let ciphertext = &raw[HEADER_LEN..];

        let key = derive_key(passphrase, &salt, params)?;

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key[..]));
        let plaintext = cipher
            .decrypt(
                Nonce::from_slice(nonce_bytes),
                Payload {
                    msg: ciphertext,
                    aad: header,
                },
            )
            // AES-GCM không phân biệt được "sai khoá" với "ciphertext bị sửa" — cả hai
            // đều là tag mismatch. Passphrase sai là nguyên nhân phổ biến gấp nhiều lần
            // nên báo cái đó; message của `Tampered` chỉ dùng khi header rõ ràng lệch.
            .map_err(|_| VaultError::WrongPassphrase)?;

        let plaintext = Zeroizing::new(plaintext);
        let contents: VaultContents = serde_json::from_slice(&plaintext)
            .map_err(|e| VaultError::Corrupt(e.to_string()))?;

        Ok(UnlockedVault {
            key,
            params,
            salt,
            contents,
        })
    }

    /// Ghi lại vault bằng khoá đã có trong `UnlockedVault` — không cần passphrase.
    pub fn save(&self, v: &UnlockedVault) -> Result<()> {
        self.write(&v.key, v.params, &v.salt, &v.contents)
    }

    fn write(
        &self,
        key: &[u8; 32],
        params: KdfParams,
        salt: &[u8; SALT_LEN],
        contents: &VaultContents,
    ) -> Result<()> {
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);

        let mut header = Vec::with_capacity(HEADER_LEN);
        header.extend_from_slice(MAGIC);
        header.push(VERSION);
        header.extend_from_slice(&params.m_cost.to_le_bytes());
        header.extend_from_slice(&params.t_cost.to_le_bytes());
        header.push(params.p_cost as u8);
        header.extend_from_slice(salt);
        header.extend_from_slice(&nonce_bytes);
        debug_assert_eq!(header.len(), HEADER_LEN);

        let plaintext = Zeroizing::new(
            serde_json::to_vec(contents).map_err(|e| VaultError::Corrupt(e.to_string()))?,
        );

        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(key));
        let ciphertext = cipher
            .encrypt(
                Nonce::from_slice(&nonce_bytes),
                Payload {
                    msg: &plaintext,
                    aad: &header,
                },
            )
            .map_err(|e| VaultError::Corrupt(format!("mã hoá thất bại: {e}")))?;

        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| VaultError::Io(e.to_string()))?;
        }

        // Ghi ra file tạm rồi rename: mất điện giữa lúc ghi sẽ không để lại vault hỏng.
        let tmp = self.path.with_extension("vault.tmp");
        let mut blob = header;
        blob.extend_from_slice(&ciphertext);
        std::fs::write(&tmp, &blob).map_err(|e| VaultError::Io(e.to_string()))?;
        std::fs::rename(&tmp, &self.path).map_err(|e| VaultError::Io(e.to_string()))?;
        Ok(())
    }
}

impl UnlockedVault {
    pub fn add(&mut self, sa_json: String) {
        self.contents.credentials.push(sa_json);
        self.contents.active_index = self.contents.credentials.len() - 1;
    }

    pub fn remove(&mut self, index: usize) -> Result<()> {
        if index >= self.contents.credentials.len() {
            return Err(VaultError::NoSuchIndex(index));
        }
        let mut removed = self.contents.credentials.remove(index);
        removed.zeroize();
        // Giữ active_index nằm trong khoảng hợp lệ; rỗng thì về 0.
        if self.contents.active_index >= self.contents.credentials.len() {
            self.contents.active_index = self.contents.credentials.len().saturating_sub(1);
        }
        Ok(())
    }

    pub fn set_active(&mut self, index: usize) -> Result<()> {
        if index >= self.contents.credentials.len() {
            return Err(VaultError::NoSuchIndex(index));
        }
        self.contents.active_index = index;
        Ok(())
    }
}

fn derive_key(passphrase: &str, salt: &[u8], params: KdfParams) -> Result<Zeroizing<[u8; 32]>> {
    let p = Params::new(params.m_cost, params.t_cost, params.p_cost, Some(32))
        .map_err(|e| VaultError::Corrupt(format!("tham số Argon2 không hợp lệ: {e}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, p);

    let mut out = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut out[..])
        .map_err(|e| VaultError::Corrupt(format!("dẫn xuất khoá thất bại: {e}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tham số nhẹ cho test. Vẫn là Argon2id thật, chỉ giảm bộ nhớ để bộ test chạy nhanh.
    /// Tham số nằm trong header file nên đổi được tự do mà không phá tương thích.
    fn fast() -> KdfParams {
        KdfParams {
            m_cost: 8 * 1024,
            t_cost: 1,
            p_cost: 1,
        }
    }

    fn tmp_vault(name: &str) -> Vault {
        let dir = std::env::temp_dir().join("crc-vault-test").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        Vault::new(dir.join("credentials.vault"))
    }

    fn contents(items: &[&str]) -> VaultContents {
        VaultContents {
            credentials: items.iter().map(|s| s.to_string()).collect(),
            active_index: 0,
        }
    }

    const SA: &str = r#"{"type":"service_account","client_email":"a@p.iam.gserviceaccount.com"}"#;

    #[test]
    fn round_trip_ra_dung_noi_dung() {
        let v = tmp_vault("roundtrip");
        assert!(!v.exists());

        v.create("mật-khẩu-đủ-dài", &contents(&[SA]), fast()).unwrap();
        assert!(v.exists());

        let un = v.unlock("mật-khẩu-đủ-dài").unwrap();
        assert_eq!(un.contents.credentials.len(), 1);
        assert_eq!(un.contents.active().map(String::as_str), Some(SA));
    }

    #[test]
    fn passphrase_sai_bao_loi_ro_khong_panic() {
        let v = tmp_vault("wrongpass");
        v.create("mật-khẩu-đủ-dài", &contents(&[SA]), fast()).unwrap();

        let err = v.unlock("mật-khẩu-sai-rồi").unwrap_err();
        assert!(matches!(err, VaultError::WrongPassphrase), "{err:?}");
        assert!(err.to_string().contains("Passphrase không đúng"));
    }

    #[test]
    fn passphrase_qua_ngan_bi_chan_ngay_luc_tao() {
        let v = tmp_vault("shortpass");
        let err = v.create("123", &contents(&[SA]), fast()).unwrap_err();
        assert!(matches!(err, VaultError::PassphraseTooShort));
        assert!(!v.exists(), "không được tạo file khi passphrase bị từ chối");
    }

    #[test]
    fn sua_mot_byte_ciphertext_thi_giai_ma_that_bai() {
        let v = tmp_vault("tamper-ct");
        v.create("mật-khẩu-đủ-dài", &contents(&[SA]), fast()).unwrap();

        let mut raw = std::fs::read(v.path()).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0x01;
        std::fs::write(v.path(), &raw).unwrap();

        assert!(
            v.unlock("mật-khẩu-đủ-dài").is_err(),
            "AEAD phải phát hiện ciphertext bị sửa"
        );
    }

    #[test]
    fn sua_tham_so_argon2_trong_header_thi_giai_ma_that_bai() {
        // Đây là lý do header phải là AAD: không có nó, kẻ có file hạ m_cost xuống 1 rồi
        // brute-force passphrase gần như miễn phí.
        let v = tmp_vault("tamper-params");
        v.create("mật-khẩu-đủ-dài", &contents(&[SA]), fast()).unwrap();

        let mut raw = std::fs::read(v.path()).unwrap();
        raw[6..10].copy_from_slice(&8u32.to_le_bytes()); // m_cost = 8 KiB
        std::fs::write(v.path(), &raw).unwrap();

        assert!(
            v.unlock("mật-khẩu-đủ-dài").is_err(),
            "hạ tham số KDF phải làm giải mã thất bại, không được im lặng chấp nhận"
        );
    }

    #[test]
    fn file_bi_cat_ngan_bao_loi_ro() {
        let v = tmp_vault("truncated");
        v.create("mật-khẩu-đủ-dài", &contents(&[SA]), fast()).unwrap();
        std::fs::write(v.path(), b"CRCV1").unwrap();
        let err = v.unlock("mật-khẩu-đủ-dài").unwrap_err();
        assert!(matches!(err, VaultError::Truncated(5)), "{err:?}");
    }

    #[test]
    fn magic_sai_bao_loi_ro() {
        let v = tmp_vault("badmagic");
        v.create("mật-khẩu-đủ-dài", &contents(&[SA]), fast()).unwrap();
        let mut raw = std::fs::read(v.path()).unwrap();
        raw[0] = b'X';
        std::fs::write(v.path(), &raw).unwrap();
        assert!(matches!(v.unlock("mật-khẩu-đủ-dài"), Err(VaultError::BadMagic)));
    }

    #[test]
    fn version_la_bao_loi_ro_thay_vi_doan() {
        let v = tmp_vault("badversion");
        v.create("mật-khẩu-đủ-dài", &contents(&[SA]), fast()).unwrap();
        let mut raw = std::fs::read(v.path()).unwrap();
        raw[5] = 99;
        std::fs::write(v.path(), &raw).unwrap();
        let err = v.unlock("mật-khẩu-đủ-dài").unwrap_err();
        assert!(matches!(err, VaultError::BadVersion(99)), "{err:?}");
    }

    #[test]
    fn vault_chua_ton_tai_thi_bao_not_found() {
        let v = tmp_vault("missing");
        assert!(matches!(v.unlock("mật-khẩu-đủ-dài"), Err(VaultError::NotFound)));
    }

    #[test]
    fn tham_so_kdf_duoc_luu_trong_file_va_doc_lai_dung() {
        let v = tmp_vault("params-persist");
        let p = KdfParams {
            m_cost: 16 * 1024,
            t_cost: 2,
            p_cost: 1,
        };
        v.create("mật-khẩu-đủ-dài", &contents(&[SA]), p).unwrap();
        let un = v.unlock("mật-khẩu-đủ-dài").unwrap();
        assert_eq!(un.params, p, "tham số phải đọc lại từ header, không hardcode");
    }

    #[test]
    fn them_va_xoa_credential_khong_can_passphrase_lai() {
        let v = tmp_vault("addremove");
        let mut un = v.create("mật-khẩu-đủ-dài", &contents(&[SA]), fast()).unwrap();

        un.add(r#"{"type":"service_account","client_email":"b@p.iam.gserviceaccount.com"}"#.into());
        assert_eq!(un.contents.active_index, 1, "thêm mới thì active chuyển sang cái mới");
        v.save(&un).unwrap();

        let re = v.unlock("mật-khẩu-đủ-dài").unwrap();
        assert_eq!(re.contents.credentials.len(), 2);
        assert_eq!(re.contents.active_index, 1);

        let mut un2 = re;
        un2.remove(1).unwrap();
        assert_eq!(un2.contents.credentials.len(), 1);
        assert_eq!(un2.contents.active_index, 0, "active_index phải được kẹp lại");
        v.save(&un2).unwrap();
        assert_eq!(v.unlock("mật-khẩu-đủ-dài").unwrap().contents.credentials.len(), 1);
    }

    #[test]
    fn xoa_het_credential_khong_lam_active_index_ra_ngoai_khoang() {
        let v = tmp_vault("removeall");
        let mut un = v.create("mật-khẩu-đủ-dài", &contents(&[SA]), fast()).unwrap();
        un.remove(0).unwrap();
        assert!(un.contents.credentials.is_empty());
        assert_eq!(un.contents.active_index, 0);
        assert!(un.contents.active().is_none());
    }

    #[test]
    fn remove_index_ngoai_khoang_bao_loi() {
        let v = tmp_vault("badindex");
        let mut un = v.create("mật-khẩu-đủ-dài", &contents(&[SA]), fast()).unwrap();
        assert!(matches!(un.remove(9), Err(VaultError::NoSuchIndex(9))));
        assert!(matches!(un.set_active(9), Err(VaultError::NoSuchIndex(9))));
    }

    #[test]
    fn debug_khong_lam_ro_khoa() {
        let v = tmp_vault("debug");
        let un = v.create("mật-khẩu-đủ-dài", &contents(&[SA]), fast()).unwrap();
        let dbg = format!("{un:?}");
        assert!(dbg.contains("redacted"));
        assert!(!dbg.contains("gserviceaccount"), "Debug rò nội dung credential: {dbg}");
    }

    #[test]
    fn nonce_khac_nhau_moi_lan_ghi() {
        // Dùng lại nonce với cùng khoá là lỗi chí tử của GCM (làm lộ plaintext XOR).
        let v = tmp_vault("nonce");
        let un = v.create("mật-khẩu-đủ-dài", &contents(&[SA]), fast()).unwrap();
        let n1 = std::fs::read(v.path()).unwrap()[31..43].to_vec();
        v.save(&un).unwrap();
        let n2 = std::fs::read(v.path()).unwrap()[31..43].to_vec();
        assert_ne!(n1, n2, "nonce phải được sinh mới mỗi lần ghi");
    }

    #[test]
    fn ghi_khong_de_lai_file_tam() {
        let v = tmp_vault("notmp");
        v.create("mật-khẩu-đủ-dài", &contents(&[SA]), fast()).unwrap();
        let tmp = v.path().with_extension("vault.tmp");
        assert!(!tmp.exists(), "file tạm phải được rename, không để lại");
    }
}
