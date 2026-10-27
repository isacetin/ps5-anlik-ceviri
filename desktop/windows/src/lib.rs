//! Windows platform katmanı: Windows.Graphics.Capture ile pencere yakalama, Windows.Media.Ocr ile
//! yazı tanıma. `WindowsScreenReader` ortak `ScreenReader` sözleşmesini uygular (Mac'te
//! `ceviri-mac`). Faz 0 test aracı `winprobe` da bu modülleri kullanır.
//!
//! `imaging` her işletim sisteminde derlenir ve test edilir; geri kalanı yalnızca Windows'ta.

pub mod imaging;

#[cfg(windows)]
pub mod capture;
#[cfg(windows)]
pub mod ocr;
#[cfg(windows)]
mod reader;
#[cfg(windows)]
pub mod win;

#[cfg(windows)]
pub use reader::WindowsScreenReader;

/// WinRT çağrılarından önce bu thread'i çok iş parçacıklı daireye (MTA) sokar. Tekrar çağrılması
/// zararsızdır (thread zaten MTA'daysa hiçbir şey olmaz).
#[cfg(windows)]
pub fn ensure_mta() {
    use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
}
