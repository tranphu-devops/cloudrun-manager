//! Wrapper cho dữ liệu nhạy cảm (access token, secret payload).
//!
//! Ba việc nó bảo đảm:
//!   1. `Debug`/`Display` không bao giờ in ra nội dung thật -> không rò qua log/panic message.
//!   2. `Drop` zeroize buffer -> giảm cửa sổ tồn tại của secret trong RAM.
//!   3. Không implement `Serialize` -> không thể vô tình gửi ngược ra frontend.
//!
//! Muốn lấy giá trị thật phải gọi `expose()` một cách có ý thức. `grep expose()`
//! là cách audit toàn bộ điểm chạm secret trong codebase.

use std::fmt;
use zeroize::Zeroize;

#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(v: impl Into<String>) -> Self {
        Self(v.into())
    }

    /// Lấy giá trị thật. Mỗi call site của hàm này là một điểm cần review bảo mật.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret([redacted; {} bytes])", self.0.len())
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_khong_lam_ro_gia_tri() {
        let s = Secret::new("ya29.super-secret-token");
        let dbg = format!("{s:?}");
        assert!(!dbg.contains("ya29"), "Debug bị rò secret: {dbg}");
        assert!(!dbg.contains("super-secret"), "Debug bị rò secret: {dbg}");
        assert!(dbg.contains("redacted"));
    }

    #[test]
    fn display_khong_lam_ro_gia_tri() {
        let s = Secret::new("hunter2");
        assert_eq!(format!("{s}"), "[redacted]");
    }

    #[test]
    fn expose_tra_ve_gia_tri_that() {
        let s = Secret::new("abc123");
        assert_eq!(s.expose(), "abc123");
        assert_eq!(s.len(), 6);
    }

    /// Secret bọc trong struct khác cũng không được rò khi struct đó derive Debug.
    #[test]
    fn secret_long_trong_struct_khac_van_an_toan() {
        #[derive(Debug)]
        struct Holder {
            #[allow(dead_code)]
            account: String,
            #[allow(dead_code)]
            token: Secret,
        }
        let h = Holder {
            account: "you@example.com".into(),
            token: Secret::new("ya29.leak-me"),
        };
        let dbg = format!("{h:?}");
        assert!(!dbg.contains("leak-me"), "rò qua struct bọc ngoài: {dbg}");
        assert!(dbg.contains("you@example.com"), "field thường vẫn phải in ra");
    }
}
